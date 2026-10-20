//! Letters: written to the outbox in the store, sent by a worker through the
//! SMTP server in `LAPI_OAUTH_SMTP_URL` once it is set. Until then they wait
//! (three days at most). A waiting letter's body is sealed with `mail.key`
//! (not in the database), so a copy of the database opens no link; a sent
//! letter keeps no body. A failed letter is tried again after a growing wait.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD as B64;
use lettre::message::{Mailbox, MultiPart, header::ContentType};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use rand::RngCore;
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};

use crate::app::{App, now};
use crate::letters::{Content, Rendered};
use crate::pages::Lang;

pub enum Letter<'a> {
    /// Confirm the address: a link valid 24 hours.
    Verify { nick: &'a str, link: &'a str },
    /// Set a new password: a link valid 30 minutes.
    Reset { nick: &'a str, link: &'a str },
    /// The password was changed; every other device was signed out.
    PasswordChanged { nick: &'a str },
    /// To the old address: the account's email is now `new` (masked).
    EmailChanged { nick: &'a str, new: &'a str },
    /// The account was deleted.
    Deleted { nick: &'a str },
}

/// `steve@example.com` as `s***@example.com`.
pub fn masked(email: &str) -> String {
    let (name, domain) = email.split_once('@').unwrap_or((email, ""));
    let first: String = name.chars().take(1).collect();
    format!("{first}***@{domain}")
}

impl Letter<'_> {
    /// Subject, plain text and HTML in the account's language.
    pub fn render(&self, lang: Lang) -> Rendered {
        self.content(lang).render(lang)
    }

    fn content(&self, lang: Lang) -> Content {
        let hi = |nick: &str| match lang {
            Lang::Ru => format!("Привет, {nick}!"),
            Lang::Ar => format!("مرحبًا {nick}!"),
            Lang::En => format!("Hi {nick},"),
        };
        let s = |en: &str, ru: &str, ar: &str| {
            match lang {
                Lang::Ru => ru,
                Lang::Ar => ar,
                Lang::En => en,
            }
            .to_owned()
        };
        match self {
            Self::Verify { nick, link } => Content {
                subject: s(
                    "Confirm your email for Sferum Launcher",
                    "Подтвердите email для Sferum Launcher",
                    "أكّد بريدك الإلكتروني في Sferum Launcher",
                ),
                preheader: s(
                    "One click, and friends and chats are open to you.",
                    "Один клик — и друзья и чаты открыты.",
                    "نقرة واحدة وتصبح الأصدقاء والمحادثات متاحة لك.",
                ),
                heading: s(
                    "Confirm your email",
                    "Подтвердите email",
                    "أكّد بريدك الإلكتروني",
                ),
                greeting: hi(nick),
                paragraphs: vec![s(
                    "Thanks for making a launcher account. Confirm this address, and friends and chats in the launcher are open to you. We also use it to help you back into the account if you forget the password.",
                    "Спасибо, что создали аккаунт лаунчера. Подтвердите этот адрес — и в лаунчере откроются друзья и чаты. Ещё он поможет вернуть доступ, если вы забудете пароль.",
                    "شكرًا لإنشاء حساب في المشغّل. أكّد هذا العنوان لتصبح الأصدقاء والمحادثات متاحة لك في المشغّل. وسيساعدك أيضًا على استعادة حسابك إذا نسيت كلمة المرور.",
                )],
                button: Some((
                    s("Confirm email", "Подтвердить email", "تأكيد البريد"),
                    (*link).to_owned(),
                )),
                after_button: Some(s(
                    "The link works for 24 hours. Then press \"Check again\" in the launcher.",
                    "Ссылка действует 24 часа. После подтверждения нажмите в лаунчере «Проверить снова».",
                    "يعمل الرابط لمدة 24 ساعة. بعد التأكيد اضغط «تحقق مرة أخرى» في المشغّل.",
                )),
                warning: Some(s(
                    "If you did not make an account, ignore this letter: without the confirmation nothing happens.",
                    "Если вы не создавали аккаунт, просто проигнорируйте письмо: без подтверждения ничего не произойдёт.",
                    "إذا لم تنشئ حسابًا، فتجاهل هذه الرسالة: لن يحدث شيء دون التأكيد.",
                )),
            },
            Self::Reset { nick, link } => Content {
                subject: s(
                    "Reset your Sferum Launcher password",
                    "Сброс пароля Sferum Launcher",
                    "إعادة تعيين كلمة مرور Sferum Launcher",
                ),
                preheader: s(
                    "A link to set a new password, valid for 30 minutes.",
                    "Ссылка, чтобы задать новый пароль. Действует 30 минут.",
                    "رابط لتعيين كلمة مرور جديدة، صالح لمدة 30 دقيقة.",
                ),
                heading: s(
                    "Set a new password",
                    "Новый пароль",
                    "تعيين كلمة مرور جديدة",
                ),
                greeting: hi(nick),
                paragraphs: vec![s(
                    "Someone (we hope you) asked to reset the password of your launcher account. Press the button and choose a new one.",
                    "Кто-то (надеемся, вы) попросил сбросить пароль аккаунта лаунчера. Нажмите кнопку и придумайте новый.",
                    "طلب أحدهم (نأمل أن تكون أنت) إعادة تعيين كلمة مرور حساب المشغّل. اضغط الزر واختر كلمة مرور جديدة.",
                )],
                button: Some((
                    s(
                        "Set a new password",
                        "Задать новый пароль",
                        "تعيين كلمة مرور جديدة",
                    ),
                    (*link).to_owned(),
                )),
                after_button: Some(s(
                    "The link works for 30 minutes and only once.",
                    "Ссылка действует 30 минут и только один раз.",
                    "يعمل الرابط لمدة 30 دقيقة ولمرة واحدة فقط.",
                )),
                warning: Some(s(
                    "If you did not ask for this, do nothing: your password stays as it is.",
                    "Если вы не просили сброс, ничего не делайте: пароль останется прежним.",
                    "إذا لم تطلب ذلك، فلا تفعل شيئًا: ستبقى كلمة المرور كما هي.",
                )),
            },
            Self::PasswordChanged { nick } => Content {
                subject: s(
                    "Your Sferum Launcher password was changed",
                    "Пароль Sferum Launcher изменён",
                    "تم تغيير كلمة مرور Sferum Launcher",
                ),
                preheader: s(
                    "Other devices were signed out.",
                    "Остальные устройства вышли из аккаунта.",
                    "تم تسجيل الخروج من الأجهزة الأخرى.",
                ),
                heading: s("Password changed", "Пароль изменён", "تم تغيير كلمة المرور"),
                greeting: hi(nick),
                paragraphs: vec![s(
                    "The password of your launcher account was just changed. Every other device was signed out of the account.",
                    "Пароль вашего аккаунта лаунчера только что изменён. Все остальные устройства вышли из аккаунта.",
                    "تم للتو تغيير كلمة مرور حساب المشغّل الخاص بك، وتم تسجيل الخروج من جميع الأجهزة الأخرى.",
                )],
                button: None,
                after_button: None,
                warning: Some(s(
                    "If this was not you, reset the password at once in the launcher (\"Forgot your password?\") and write to support.",
                    "Если это были не вы, сразу сбросьте пароль в лаунчере («Забыли пароль?») и напишите в поддержку.",
                    "إذا لم تكن أنت، فأعد تعيين كلمة المرور فورًا في المشغّل («نسيت كلمة المرور؟») وراسل الدعم.",
                )),
            },
            Self::EmailChanged { nick, new } => Content {
                subject: s(
                    "The email of your Sferum Launcher account was changed",
                    "Email аккаунта Sferum Launcher изменён",
                    "تم تغيير البريد الإلكتروني لحساب Sferum Launcher",
                ),
                preheader: s(
                    "Letters now go to the new address.",
                    "Письма теперь приходят на новый адрес.",
                    "ستصل الرسائل الآن إلى العنوان الجديد.",
                ),
                heading: s("Email changed", "Email изменён", "تم تغيير البريد"),
                greeting: hi(nick),
                paragraphs: vec![match lang {
                    Lang::Ru => format!(
                        "Email вашего аккаунта лаунчера только что изменён на {new}. Письма теперь приходят туда, а это письмо — последнее на этот адрес."
                    ),
                    Lang::Ar => format!(
                        "تم للتو تغيير البريد الإلكتروني لحسابك إلى {new}. ستصل الرسائل إليه من الآن، وهذه آخر رسالة إلى هذا العنوان."
                    ),
                    Lang::En => format!(
                        "The email of your launcher account was just changed to {new}. Letters now go there; this is the last one to this address."
                    ),
                }],
                button: None,
                after_button: None,
                warning: Some(s(
                    "If this was not you, change the password in the launcher at once and write to support.",
                    "Если это были не вы, сразу смените пароль в лаунчере и напишите в поддержку.",
                    "إذا لم تكن أنت، فغيّر كلمة المرور في المشغّل فورًا وراسل الدعم.",
                )),
            },
            Self::Deleted { nick } => Content {
                subject: s(
                    "Your Sferum Launcher account was deleted",
                    "Аккаунт Sferum Launcher удалён",
                    "تم حذف حساب Sferum Launcher",
                ),
                preheader: s(
                    "The account, its friends and chats are gone.",
                    "Аккаунт, его друзья и чаты удалены.",
                    "تم حذف الحساب وأصدقائه ومحادثاته.",
                ),
                heading: s("Account deleted", "Аккаунт удалён", "تم حذف الحساب"),
                greeting: hi(nick),
                paragraphs: vec![s(
                    "Your launcher account, its friends and chats were deleted, as asked. This is the last letter we send you. Thank you for playing with us.",
                    "Ваш аккаунт лаунчера, его друзья и чаты удалены, как вы и просили. Это последнее письмо от нас. Спасибо, что играли с нами.",
                    "تم حذف حساب المشغّل الخاص بك مع أصدقائه ومحادثاته كما طلبت. هذه آخر رسالة نرسلها إليك. شكرًا لأنك لعبت معنا.",
                )],
                button: None,
                after_button: None,
                warning: Some(s(
                    "If you did not delete it, write to support at once: someone knew your password.",
                    "Если вы его не удаляли, сразу напишите в поддержку: кто-то знал ваш пароль.",
                    "إذا لم تحذفه بنفسك، فراسل الدعم فورًا: شخص ما كان يعرف كلمة مرورك.",
                )),
            },
        }
    }
}

/// Seals the bodies of waiting letters (ChaCha20-Poly1305, bound to the
/// recipient).
pub struct Sealer(LessSafeKey);

/// Marks a sealed body; a body without it is from before sealing.
const SEALED: &str = "sealed:1:";

impl Sealer {
    pub fn new(key: [u8; 32]) -> Self {
        Self(LessSafeKey::new(
            UnboundKey::new(&CHACHA20_POLY1305, &key).expect("a 32-byte key"),
        ))
    }

    /// A key for this run only (tests).
    pub fn random() -> Self {
        let mut key = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut key);
        Self::new(key)
    }

    pub fn seal(&self, to: &str, body: &str) -> String {
        let mut nonce = [0u8; NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let mut sealed = body.as_bytes().to_vec();
        self.0
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(to.as_bytes()),
                &mut sealed,
            )
            .expect("sealing fits in memory");
        let mut out = nonce.to_vec();
        out.extend_from_slice(&sealed);
        format!("{SEALED}{}", B64.encode(out))
    }

    /// The body as written; None when it was sealed with another key or for
    /// another recipient.
    pub fn open(&self, to: &str, stored: &str) -> Option<String> {
        let Some(sealed) = stored.strip_prefix(SEALED) else {
            return Some(stored.to_owned());
        };
        let mut bytes = B64.decode(sealed).ok()?;
        if bytes.len() < NONCE_LEN {
            return None;
        }
        let mut data = bytes.split_off(NONCE_LEN);
        let nonce = Nonce::try_assume_unique_for_key(&bytes).ok()?;
        let plain = self
            .0
            .open_in_place(nonce, Aad::from(to.as_bytes()), &mut data)
            .ok()?;
        String::from_utf8(plain.to_vec()).ok()
    }
}

/// Puts a letter in the outbox.
pub fn queue(app: &App, to: &str, lang: &str, letter: &Letter<'_>) {
    let letter = letter.render(Lang::parse(lang));
    let body = app.sealer.seal(to, &letter.text);
    let html = app.sealer.seal(to, &letter.html);
    if let Err(e) = app
        .store
        .queue_mail(to, &letter.subject, &body, &html, now())
    {
        eprintln!("queueing a letter failed: {e}");
    }
}

/// Seals letters queued before bodies were sealed.
pub fn seal_waiting(app: &App) -> Result<(), String> {
    for (id, to, body) in app.store.unsealed_mail()? {
        if !body.starts_with(SEALED) {
            app.store.set_mail_body(id, &app.sealer.seal(&to, &body))?;
        }
    }
    Ok(())
}

/// Letters due now, opened; one that cannot be opened is dropped.
pub fn due(app: &App, now: i64, limit: i64) -> Result<Vec<crate::store::Mail>, String> {
    let mut due = Vec::new();
    for mut mail in app.store.pending_mail(now, limit)? {
        // A letter from before HTML parts has none; it goes as text.
        let html = if mail.html.is_empty() {
            Some(String::new())
        } else {
            app.sealer.open(&mail.to, &mail.html)
        };
        match (app.sealer.open(&mail.to, &mail.body), html) {
            (Some(body), Some(html)) => {
                mail.body = body;
                mail.html = html;
                due.push(mail);
            }
            _ => {
                eprintln!("letter {} cannot be opened with mail.key; dropped", mail.id);
                app.store.mail_dropped(mail.id)?;
            }
        }
    }
    Ok(due)
}

pub struct Smtp {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl Smtp {
    /// From `smtps://user:password@host:465` (or `smtp://…?tls=required`) and
    /// a sender such as `Launcher <no-reply@lapi.sh>`.
    pub fn new(url: &str, from: &str) -> Result<Self, String> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
            .map_err(|_| "LAPI_OAUTH_SMTP_URL is not a valid SMTP address".to_owned())?
            .timeout(Some(Duration::from_secs(20)))
            .build();
        let from = from
            .parse()
            .map_err(|_| "LAPI_OAUTH_MAIL_FROM is not a valid sender".to_owned())?;
        Ok(Self { transport, from })
    }

    /// Connects, starts TLS and signs in, sending nothing.
    pub async fn sign_in(&self) -> Result<(), String> {
        match self.transport.test_connection().await {
            Ok(true) => Ok(()),
            Ok(false) => Err("the server closed the connection".to_owned()),
            Err(e) => Err(failure(&e)),
        }
    }

    /// A short test letter to `to`, or to the sender's own address.
    pub async fn send_test(&self, to: Option<&str>) -> Result<(), String> {
        let to = match to {
            Some(to) => to.parse().map_err(|_| "not an address".to_owned())?,
            None => self.from.clone(),
        };
        // The same look as the real letters, so the check shows it too.
        let letter = Content {
            subject: "Test letter from the account service".into(),
            preheader: "Letters go out.".into(),
            heading: "Letters go out".into(),
            greeting: "Hello,".into(),
            paragraphs: vec![
                "This is a test letter from the mail check: the account service can send letters through its SMTP server.".into(),
            ],
            button: None,
            after_button: None,
            warning: None,
        }
        .render(Lang::En);
        let message = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(letter.subject)
            .multipart(MultiPart::alternative_plain_html(letter.text, letter.html))
            .map_err(|_| "the letter could not be made".to_owned())?;
        self.transport
            .send(message)
            .await
            .map(drop)
            .map_err(|e| failure(&e))
    }

    async fn send(&self, mail: &crate::store::Mail) -> Result<(), String> {
        let message = Message::builder()
            .from(self.from.clone())
            .to(mail.to.parse().map_err(|_| "bad recipient".to_owned())?)
            .subject(mail.subject.clone());
        let message = if mail.html.is_empty() {
            message
                .header(ContentType::TEXT_PLAIN)
                .body(mail.body.clone())
        } else {
            message.multipart(MultiPart::alternative_plain_html(
                mail.body.clone(),
                mail.html.clone(),
            ))
        }
        .map_err(|_| "the letter could not be made".to_owned())?;
        // Stored with the letter and shown by `mail-check`, so in the same
        // public words as there.
        self.transport
            .send(message)
            .await
            .map(drop)
            .map_err(|e| failure(&e))
    }
}

/// A stored failure as `mail-check` may print it: the fixed words this
/// service writes as they are; anything else (written before, holding the
/// server's text) only as its SMTP reply code.
pub fn public_failure(stored: &str) -> String {
    const FIXED: [&str; 9] = [
        "bad recipient",
        "refused",
        "refused for now",
        "the letter could not be made",
        "no answer in time",
        "TLS failed",
        "the conversation with the server failed",
        "no connection to the server",
        "cannot be opened",
    ];
    if FIXED.contains(&stored) {
        return stored.to_owned();
    }
    let coded = |prefix: &str| {
        let code = stored
            .strip_prefix(prefix)?
            .strip_prefix(" (")?
            .strip_suffix(')')?;
        smtp_code(code).then(|| stored.to_owned())
    };
    if let Some(words) = coded("refused for now").or_else(|| coded("refused")) {
        return words;
    }
    // lettre writes a reply as `permanent error (550): …`.
    ["permanent error (", "transient error ("]
        .iter()
        .find_map(|p| stored.split_once(p)?.1.get(..4))
        .and_then(|c| c.strip_suffix(')').filter(|c| smtp_code(c)))
        .map_or_else(
            || "unknown".to_owned(),
            |code| format!("the server answered {code}"),
        )
}

/// A reply code that refuses: 4xx or 5xx.
fn smtp_code(word: &str) -> bool {
    word.len() == 3 && word.starts_with(['4', '5']) && word.bytes().all(|b| b.is_ascii_digit())
}

/// An SMTP failure in words fit for a public log: what failed and the
/// server's reply code, never its text, which may name an address or a host.
fn failure(e: &lettre::transport::smtp::Error) -> String {
    let code = e.status().map(|c| format!(" ({c})")).unwrap_or_default();
    if e.is_timeout() {
        "no answer in time".to_owned()
    } else if e.is_tls() {
        "TLS failed".to_owned()
    } else if e.is_permanent() {
        format!("refused{code}")
    } else if e.is_transient() {
        format!("refused for now{code}")
    } else if e.is_client() || e.is_response() {
        "the conversation with the server failed".to_owned()
    } else {
        "no connection to the server".to_owned()
    }
}

/// A value from a systemd environment file (`KEY="value"` with `\"`, `\\`,
/// `\``, `\$` escaped, as deploy.sh writes it, or `KEY=value`).
pub fn env_file_value(text: &str, key: &str) -> Option<String> {
    let line = text
        .lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))?;
    let Some(quoted) = line.strip_prefix('"').and_then(|l| l.strip_suffix('"')) else {
        return Some(line.trim().to_owned()).filter(|v| !v.is_empty());
    };
    let mut value = String::new();
    let mut chars = quoted.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => value.extend(chars.next()),
            c => value.push(c),
        }
    }
    Some(value).filter(|v| !v.is_empty())
}

/// Sends what waits in the outbox every 20 seconds.
pub async fn worker(app: Arc<App>, smtp: Smtp) {
    loop {
        let pending = {
            let app = Arc::clone(&app);
            tokio::task::spawn_blocking(move || due(&app, now(), 20)).await
        };
        if let Ok(Ok(pending)) = pending {
            for mail in pending {
                let result = smtp.send(&mail).await;
                if let Err(e) = &result {
                    // The error names the server's answer, never the recipient.
                    eprintln!("sending letter {} failed: {e}", mail.id);
                }
                let _ =
                    app.store
                        .mail_done(mail.id, result.as_ref().err().map(String::as_str), now());
            }
        }
        tokio::time::sleep(Duration::from_secs(20)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_file_values_are_read_as_systemd_reads_them() {
        let text = "LAPI_OAUTH_SMTP_URL=\"smtps://u:p%40\\\"\\\\\\$x@host:465\"\n\
                    LAPI_OAUTH_MAIL_FROM=Launcher <no-reply@example.com>\n\
                    LAPI_OAUTH_SMTP=\"\"\n";
        assert_eq!(
            env_file_value(text, "LAPI_OAUTH_SMTP_URL").as_deref(),
            Some("smtps://u:p%40\"\\$x@host:465")
        );
        assert_eq!(
            env_file_value(text, "LAPI_OAUTH_MAIL_FROM").as_deref(),
            Some("Launcher <no-reply@example.com>")
        );
        assert_eq!(env_file_value(text, "LAPI_OAUTH_SMTP"), None);
        assert_eq!(env_file_value(text, "LAPI_OAUTH_TURNSTILE_SECRET"), None);
    }

    #[test]
    fn failures_are_shown_without_the_servers_text() {
        assert_eq!(public_failure("bad recipient"), "bad recipient");
        assert_eq!(public_failure("refused (550)"), "refused (550)");
        assert_eq!(
            public_failure("refused for now (451)"),
            "refused for now (451)"
        );
        assert_eq!(
            public_failure("permanent error (550): 5.1.1 <steve@example.com>: user unknown"),
            "the server answered 550"
        );
        assert_eq!(public_failure("refused (steve@example.com)"), "unknown");
        assert_eq!(
            public_failure("host mail.example.com:465 refused"),
            "unknown"
        );
    }

    #[test]
    fn letters_escape_what_comes_from_accounts() {
        let nick = "<b>Steve</b>&\"'";
        let link = "https://oauth.example/verify?token=a\"><script>&lang=ru";
        let letters = [
            Letter::Verify { nick, link },
            Letter::Reset { nick, link },
            Letter::PasswordChanged { nick },
            Letter::EmailChanged {
                nick,
                new: "<i>s***@example.com</i>",
            },
            Letter::Deleted { nick },
        ];
        for letter in &letters {
            for lang in [Lang::En, Lang::Ru, Lang::Ar] {
                let rendered = letter.render(lang);
                assert!(!rendered.html.contains("<b>"), "{}", rendered.subject);
                assert!(!rendered.html.contains("<script>"));
                assert!(!rendered.html.contains("<i>"));
                assert!(
                    rendered
                        .html
                        .contains("&lt;b&gt;Steve&lt;/b&gt;&amp;&quot;&#39;")
                );
                // The plain part keeps the words as they are.
                assert!(rendered.text.contains(nick));
                assert!(rendered.html.contains(&format!("lang=\"{}\"", lang.tag())));
                assert_eq!(rendered.html.contains("dir=\"rtl\""), lang == Lang::Ar);
                assert!(!rendered.subject.is_empty());
            }
        }
        let verify = Letter::Verify {
            nick: "Steve",
            link: "https://oauth.example/verify?token=t&lang=ru",
        }
        .render(Lang::Ru);
        assert_eq!(verify.subject, "Подтвердите email для Sferum Launcher");
        assert!(
            verify
                .text
                .contains("https://oauth.example/verify?token=t&lang=ru")
        );
        assert!(
            verify
                .html
                .contains("href=\"https://oauth.example/verify?token=t&amp;lang=ru\"")
        );
        assert!(verify.html.contains("Подтвердить email"));
    }

    #[test]
    fn a_test_letter_needs_a_valid_sender() {
        assert!(
            Smtp::new(
                "smtps://u:p@localhost:465",
                "Launcher <no-reply@example.com>"
            )
            .is_ok()
        );
        assert!(Smtp::new("smtps://u:p@localhost:465", "not an address").is_err());
        assert!(Smtp::new("ftp://localhost", "no-reply@example.com").is_err());
    }
}
