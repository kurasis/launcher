//! Letters: written to the outbox in the store, sent by a worker through the
//! SMTP server in `LAPI_OAUTH_SMTP_URL` once it is set. Until then they wait
//! (three days at most). A waiting letter's body is sealed with `mail.key`
//! (not in the database), so a copy of the database opens no link; a sent
//! letter keeps no body. A failed letter is tried again after a growing wait.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD as B64;
use lettre::message::{Mailbox, header::ContentType};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use rand::RngCore;
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};

use crate::app::{App, now};
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
    /// Subject and plain-text body in the account's language.
    pub fn render(&self, lang: Lang) -> (String, String) {
        match (self, lang) {
            (Self::Verify { nick, link }, Lang::Ru) => (
                "Подтвердите email аккаунта лаунчера".into(),
                format!(
                    "Привет, {nick}!\n\nПодтвердите адрес, открыв ссылку (действует 24 часа):\n{link}\n\nЕсли вы не создавали аккаунт, просто удалите это письмо.\n"
                ),
            ),
            (Self::Verify { nick, link }, Lang::Ar) => (
                "أكّد البريد الإلكتروني لحساب المشغّل".into(),
                format!(
                    "مرحبًا {nick}!\n\nأكّد عنوانك بفتح هذا الرابط (صالح 24 ساعة):\n{link}\n\nإذا لم تنشئ حسابًا، احذف هذه الرسالة.\n"
                ),
            ),
            (Self::Verify { nick, link }, Lang::En) => (
                "Confirm the email of your launcher account".into(),
                format!(
                    "Hi {nick}!\n\nConfirm your address by opening this link (it works for 24 hours):\n{link}\n\nIf you did not make an account, delete this letter.\n"
                ),
            ),
            (Self::Reset { nick, link }, Lang::Ru) => (
                "Сброс пароля аккаунта лаунчера".into(),
                format!(
                    "Привет, {nick}!\n\nЧтобы задать новый пароль, откройте ссылку (действует 30 минут):\n{link}\n\nЕсли вы не просили сброс, ничего не делайте: пароль останется прежним.\n"
                ),
            ),
            (Self::Reset { nick, link }, Lang::Ar) => (
                "إعادة تعيين كلمة مرور حساب المشغّل".into(),
                format!(
                    "مرحبًا {nick}!\n\nلتعيين كلمة مرور جديدة افتح هذا الرابط (صالح 30 دقيقة):\n{link}\n\nإذا لم تطلب ذلك، فلا تفعل شيئًا وستبقى كلمة المرور كما هي.\n"
                ),
            ),
            (Self::Reset { nick, link }, Lang::En) => (
                "Reset the password of your launcher account".into(),
                format!(
                    "Hi {nick}!\n\nTo set a new password, open this link (it works for 30 minutes):\n{link}\n\nIf you did not ask for this, do nothing: the password stays as it is.\n"
                ),
            ),
            (Self::PasswordChanged { nick }, Lang::Ru) => (
                "Пароль аккаунта лаунчера изменён".into(),
                format!(
                    "Привет, {nick}!\n\nПароль вашего аккаунта только что изменён, остальные устройства вышли из него.\nЕсли это были не вы, сразу сбросьте пароль в лаунчере («Забыли пароль?») и напишите в поддержку.\n"
                ),
            ),
            (Self::PasswordChanged { nick }, Lang::Ar) => (
                "تم تغيير كلمة مرور حساب المشغّل".into(),
                format!(
                    "مرحبًا {nick}!\n\nتم للتو تغيير كلمة مرور حسابك وتسجيل الخروج من الأجهزة الأخرى.\nإذا لم تكن أنت، فأعد تعيين كلمة المرور فورًا وراسل الدعم.\n"
                ),
            ),
            (Self::EmailChanged { nick, new }, Lang::Ru) => (
                "Email аккаунта лаунчера изменён".into(),
                format!(
                    "Привет, {nick}!\n\nEmail вашего аккаунта только что изменён на {new}. Письма теперь приходят туда.\nЕсли это были не вы, сразу смените пароль в лаунчере и напишите в поддержку.\n"
                ),
            ),
            (Self::EmailChanged { nick, new }, Lang::Ar) => (
                "تم تغيير البريد الإلكتروني لحساب المشغّل".into(),
                format!(
                    "مرحبًا {nick}!\n\nتم للتو تغيير البريد الإلكتروني لحسابك إلى {new}، وستصل الرسائل إليه من الآن.\nإذا لم تكن أنت، فغيّر كلمة المرور فورًا وراسل الدعم.\n"
                ),
            ),
            (Self::EmailChanged { nick, new }, Lang::En) => (
                "The email of your launcher account was changed".into(),
                format!(
                    "Hi {nick}!\n\nThe email of your account was just changed to {new}. Letters now go there.\nIf this was not you, change the password at once and write to support.\n"
                ),
            ),
            (Self::Deleted { nick }, Lang::Ru) => (
                "Аккаунт лаунчера удалён".into(),
                format!(
                    "Привет, {nick}!\n\nВаш аккаунт лаунчера, его друзья и чаты удалены. Это письмо последнее.\nЕсли вы его не удаляли, напишите в поддержку сразу: кто-то знал ваш пароль.\n"
                ),
            ),
            (Self::Deleted { nick }, Lang::Ar) => (
                "تم حذف حساب المشغّل".into(),
                format!(
                    "مرحبًا {nick}!\n\nتم حذف حساب المشغّل الخاص بك مع الأصدقاء والمحادثات. هذه آخر رسالة.\nإذا لم تحذفه بنفسك فراسل الدعم فورًا: شخص ما كان يعرف كلمة مرورك.\n"
                ),
            ),
            (Self::Deleted { nick }, Lang::En) => (
                "Your launcher account was deleted".into(),
                format!(
                    "Hi {nick}!\n\nYour launcher account, its friends and chats were deleted. This is the last letter.\nIf you did not delete it, write to support at once: someone knew your password.\n"
                ),
            ),
            (Self::PasswordChanged { nick }, Lang::En) => (
                "The password of your launcher account was changed".into(),
                format!(
                    "Hi {nick}!\n\nThe password of your account was just changed and other devices were signed out.\nIf this was not you, reset the password at once (\"Forgot your password?\") and write to support.\n"
                ),
            ),
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
    let (subject, body) = letter.render(Lang::parse(lang));
    let body = app.sealer.seal(to, &body);
    if let Err(e) = app.store.queue_mail(to, &subject, &body, now()) {
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
        match app.sealer.open(&mail.to, &mail.body) {
            Some(body) => {
                mail.body = body;
                due.push(mail);
            }
            None => {
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
        let message = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject("Test letter")
            .header(ContentType::TEXT_PLAIN)
            .body("The account service can send letters.\n".to_owned())
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
            .subject(mail.subject.clone())
            .header(ContentType::TEXT_PLAIN)
            .body(mail.body.clone())
            .map_err(|e| e.to_string())?;
        self.transport
            .send(message)
            .await
            .map(drop)
            .map_err(|e| e.to_string())
    }
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
