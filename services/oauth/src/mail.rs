//! Letters: written to the outbox in the store, sent by a worker through the
//! SMTP server in `LAPI_OAUTH_SMTP_URL` once it is set. Until then they wait
//! (three days at most).

use std::sync::Arc;
use std::time::Duration;

use lettre::message::{Mailbox, header::ContentType};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::app::{App, now};
use crate::pages::Lang;

pub enum Letter<'a> {
    /// Confirm the address: a link valid 24 hours.
    Verify { nick: &'a str, link: &'a str },
    /// Set a new password: a link valid 30 minutes.
    Reset { nick: &'a str, link: &'a str },
    /// The password was changed; every other device was signed out.
    PasswordChanged { nick: &'a str },
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
            (Self::PasswordChanged { nick }, Lang::En) => (
                "The password of your launcher account was changed".into(),
                format!(
                    "Hi {nick}!\n\nThe password of your account was just changed and other devices were signed out.\nIf this was not you, reset the password at once (\"Forgot your password?\") and write to support.\n"
                ),
            ),
        }
    }
}

/// Puts a letter in the outbox.
pub fn queue(app: &App, to: &str, lang: &str, letter: &Letter<'_>) {
    let (subject, body) = letter.render(Lang::parse(lang));
    if let Err(e) = app.store.queue_mail(to, &subject, &body, now()) {
        eprintln!("queueing a letter failed: {e}");
    }
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

/// Sends what waits in the outbox every 20 seconds.
pub async fn worker(app: Arc<App>, smtp: Smtp) {
    loop {
        let pending = {
            let app = Arc::clone(&app);
            tokio::task::spawn_blocking(move || app.store.pending_mail(now(), 20)).await
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
