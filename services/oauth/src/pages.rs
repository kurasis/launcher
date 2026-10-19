//! The pages of oauth.lapi.sh: sign-in, registration, password reset and
//! email confirmation, in English, Russian and Arabic.
//!
//! Everything put into a page goes through [`esc`]. Pages carry no script of
//! their own; the only one is Cloudflare Turnstile's, which the headers in
//! [`headers`] allow and nothing else.

use axum::http::{HeaderValue, header};
use axum::response::{Html, IntoResponse, Response};

use crate::rules::Problem;

/// HTML text and attribute values: the five characters that end either.
pub fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

pub const TURNSTILE_SCRIPT: &str = "https://challenges.cloudflare.com/turnstile/v0/api.js";

/// Headers of every page: no framing, no other origin's scripts, styles or
/// forms, no referrer, nothing cached.
pub fn headers(mut response: Response) -> Response {
    let h = response.headers_mut();
    let csp = "default-src 'none'; script-src https://challenges.cloudflare.com; \
               frame-src https://challenges.cloudflare.com; connect-src https://challenges.cloudflare.com; \
               style-src 'self'; img-src 'self' data:; form-action 'self'; base-uri 'none'; \
               frame-ancestors 'none'";
    for (name, value) in [
        (header::CONTENT_SECURITY_POLICY, csp),
        (header::X_FRAME_OPTIONS, "DENY"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::CACHE_CONTROL, "no-store"),
    ] {
        h.insert(name, HeaderValue::from_static(value));
    }
    response
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Ru,
    Ar,
}

impl Lang {
    pub fn parse(tag: &str) -> Self {
        match tag.get(..2).unwrap_or_default() {
            "ru" => Self::Ru,
            "ar" => Self::Ar,
            _ => Self::En,
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ru => "ru",
            Self::Ar => "ar",
        }
    }

    fn dir(self) -> &'static str {
        if self == Self::Ar { "rtl" } else { "ltr" }
    }
}

/// The texts of the pages.
pub struct T {
    pub title: &'static str,
    pub sign_in: &'static str,
    pub register: &'static str,
    pub login: &'static str,
    pub nick: &'static str,
    pub nick_hint: &'static str,
    pub email: &'static str,
    pub password: &'static str,
    pub password_hint: &'static str,
    pub password_repeat: &'static str,
    pub no_account: &'static str,
    pub have_account: &'static str,
    pub forgot: &'static str,
    pub wrong: &'static str,
    pub disabled: &'static str,
    pub captcha: &'static str,
    pub too_many: &'static str,
    pub expired: &'static str,
    pub failed: &'static str,
    pub done: &'static str,
    pub reset_title: &'static str,
    pub reset_text: &'static str,
    pub reset_send: &'static str,
    pub reset_sent: &'static str,
    pub reset_new: &'static str,
    pub reset_save: &'static str,
    pub reset_done: &'static str,
    pub link_bad: &'static str,
    pub verified: &'static str,
    pub problem: fn(Problem) -> &'static str,
}

const EN: T = T {
    title: "Launcher account",
    sign_in: "Sign in",
    register: "Create account",
    login: "Nick or email",
    nick: "Nick",
    nick_hint: "3–16 letters A–Z, digits and _. Others see it and invite you by it.",
    email: "Email",
    password: "Password",
    password_hint: "At least 10 characters.",
    password_repeat: "Password again",
    no_account: "No account yet?",
    have_account: "Already have an account?",
    forgot: "Forgot your password?",
    wrong: "The nick, email or password is not right.",
    disabled: "This account is switched off. Write to support to get it back.",
    captcha: "Confirm that you are not a robot.",
    too_many: "Too many attempts. Wait a few minutes and try again.",
    expired: "This page is out of date. Close the window and try again from the launcher.",
    failed: "Something went wrong on our side. Try again later.",
    done: "Done. You can close this window.",
    reset_title: "Reset password",
    reset_text: "Enter your email. If an account has it, we send a link to set a new password.",
    reset_send: "Send link",
    reset_sent: "If an account has this email, a letter with a link is on its way. The link works for 30 minutes.",
    reset_new: "New password",
    reset_save: "Save password",
    reset_done: "The password is changed and every device is signed out. Sign in again in the launcher.",
    link_bad: "This link has expired or was used already.",
    verified: "Your email is confirmed. You can close this page.",
    problem: |p| match p {
        Problem::NickLength => "The nick must be 3 to 16 characters.",
        Problem::NickCharacters => "The nick may only have letters A–Z, digits and _.",
        Problem::NickReserved => "This nick is reserved.",
        Problem::NickTaken => "This nick is taken.",
        Problem::Email => "This is not an email address.",
        Problem::EmailTaken => "An account with this email exists. Sign in, or reset its password.",
        Problem::PasswordShort => "The password must be at least 10 characters.",
        Problem::PasswordLong => "The password must be at most 128 characters.",
        Problem::PasswordCommon => "This password is too common. Choose another.",
        Problem::PasswordLikeNick => "The password must not contain your nick or email.",
        Problem::PasswordsDiffer => "The passwords are not the same.",
    },
};

const RU: T = T {
    title: "Аккаунт лаунчера",
    sign_in: "Войти",
    register: "Создать аккаунт",
    login: "Ник или email",
    nick: "Ник",
    nick_hint: "3–16 символов: латинские буквы, цифры и _. По нику вас видят и приглашают.",
    email: "Email",
    password: "Пароль",
    password_hint: "Не меньше 10 символов.",
    password_repeat: "Пароль ещё раз",
    no_account: "Нет аккаунта?",
    have_account: "Уже есть аккаунт?",
    forgot: "Забыли пароль?",
    wrong: "Ник, email или пароль неверны.",
    disabled: "Этот аккаунт отключён. Напишите в поддержку, чтобы его вернуть.",
    captcha: "Подтвердите, что вы не робот.",
    too_many: "Слишком много попыток. Подождите несколько минут и попробуйте снова.",
    expired: "Страница устарела. Закройте окно и начните снова из лаунчера.",
    failed: "Что-то пошло не так на нашей стороне. Попробуйте позже.",
    done: "Готово. Окно можно закрыть.",
    reset_title: "Сброс пароля",
    reset_text: "Введите email. Если он есть у аккаунта, мы пришлём ссылку для нового пароля.",
    reset_send: "Отправить ссылку",
    reset_sent: "Если такой email есть у аккаунта, письмо со ссылкой уже в пути. Ссылка действует 30 минут.",
    reset_new: "Новый пароль",
    reset_save: "Сохранить пароль",
    reset_done: "Пароль изменён, все устройства вышли из аккаунта. Войдите снова в лаунчере.",
    link_bad: "Ссылка устарела или уже использована.",
    verified: "Email подтверждён. Страницу можно закрыть.",
    problem: |p| match p {
        Problem::NickLength => "Ник должен быть от 3 до 16 символов.",
        Problem::NickCharacters => "В нике можно только латинские буквы, цифры и _.",
        Problem::NickReserved => "Этот ник зарезервирован.",
        Problem::NickTaken => "Этот ник уже занят.",
        Problem::Email => "Это не похоже на адрес email.",
        Problem::EmailTaken => "Аккаунт с этим email уже есть. Войдите или сбросьте его пароль.",
        Problem::PasswordShort => "Пароль должен быть не короче 10 символов.",
        Problem::PasswordLong => "Пароль должен быть не длиннее 128 символов.",
        Problem::PasswordCommon => "Этот пароль слишком распространён. Выберите другой.",
        Problem::PasswordLikeNick => "Пароль не должен содержать ник или email.",
        Problem::PasswordsDiffer => "Пароли не совпадают.",
    },
};

const AR: T = T {
    title: "حساب المشغّل",
    sign_in: "تسجيل الدخول",
    register: "إنشاء حساب",
    login: "الاسم المستعار أو البريد الإلكتروني",
    nick: "الاسم المستعار",
    nick_hint: "من 3 إلى 16 حرفًا: حروف لاتينية وأرقام و _. يراك الآخرون ويدعونك به.",
    email: "البريد الإلكتروني",
    password: "كلمة المرور",
    password_hint: "10 أحرف على الأقل.",
    password_repeat: "كلمة المرور مرة أخرى",
    no_account: "ليس لديك حساب؟",
    have_account: "لديك حساب بالفعل؟",
    forgot: "نسيت كلمة المرور؟",
    wrong: "الاسم المستعار أو البريد أو كلمة المرور غير صحيحة.",
    disabled: "هذا الحساب معطّل. راسل الدعم لاستعادته.",
    captcha: "أكّد أنك لست روبوتًا.",
    too_many: "محاولات كثيرة جدًا. انتظر بضع دقائق ثم حاول مرة أخرى.",
    expired: "هذه الصفحة قديمة. أغلق النافذة وابدأ من جديد من المشغّل.",
    failed: "حدث خطأ من جهتنا. حاول لاحقًا.",
    done: "تم. يمكنك إغلاق هذه النافذة.",
    reset_title: "إعادة تعيين كلمة المرور",
    reset_text: "أدخل بريدك الإلكتروني. إذا كان لحساب، نرسل رابطًا لتعيين كلمة مرور جديدة.",
    reset_send: "أرسل الرابط",
    reset_sent: "إذا كان هذا البريد لحساب، فالرسالة مع الرابط في الطريق. يعمل الرابط 30 دقيقة.",
    reset_new: "كلمة المرور الجديدة",
    reset_save: "احفظ كلمة المرور",
    reset_done: "تم تغيير كلمة المرور وتسجيل الخروج من كل الأجهزة. سجّل الدخول من جديد في المشغّل.",
    link_bad: "انتهت صلاحية هذا الرابط أو استُخدم من قبل.",
    verified: "تم تأكيد بريدك الإلكتروني. يمكنك إغلاق هذه الصفحة.",
    problem: |p| match p {
        Problem::NickLength => "يجب أن يكون الاسم المستعار من 3 إلى 16 حرفًا.",
        Problem::NickCharacters => "الاسم المستعار يقبل الحروف اللاتينية والأرقام و _ فقط.",
        Problem::NickReserved => "هذا الاسم المستعار محجوز.",
        Problem::NickTaken => "هذا الاسم المستعار مستخدم.",
        Problem::Email => "هذا ليس عنوان بريد إلكتروني.",
        Problem::EmailTaken => "يوجد حساب بهذا البريد. سجّل الدخول أو أعد تعيين كلمة مروره.",
        Problem::PasswordShort => "يجب ألا تقل كلمة المرور عن 10 أحرف.",
        Problem::PasswordLong => "يجب ألا تزيد كلمة المرور عن 128 حرفًا.",
        Problem::PasswordCommon => "كلمة المرور هذه شائعة جدًا. اختر غيرها.",
        Problem::PasswordLikeNick => "يجب ألا تحتوي كلمة المرور على اسمك المستعار أو بريدك.",
        Problem::PasswordsDiffer => "كلمتا المرور غير متطابقتين.",
    },
};

pub fn texts(lang: Lang) -> &'static T {
    match lang {
        Lang::En => &EN,
        Lang::Ru => &RU,
        Lang::Ar => &AR,
    }
}

pub const STYLE: &str = r#":root{--bg:#eef3f6;--card:#fff;--text:#14212b;--muted:#556570;--line:#d3dde3;--accent:#0f7f5f;--accent2:#0d6b8c;--danger:#b3261e;--danger-bg:#fdecea}
@media (prefers-color-scheme:dark){:root{--bg:#10171c;--card:#182229;--text:#e6edf1;--muted:#9aabb6;--line:#2c3a43;--accent:#2fbf8f;--accent2:#3aa7d0;--danger:#ff8a80;--danger-bg:#3a1f1d}}
*{box-sizing:border-box}html,body{margin:0;background:var(--bg);color:var(--text);font:16px/1.5 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif}
main{max-width:420px;margin:0 auto;padding:32px 20px}
.card{background:var(--card);border:1px solid var(--line);border-radius:14px;padding:24px}
h1{font-size:22px;margin:0 0 4px}.sub{color:var(--muted);margin:0 0 20px;font-size:14px}
label{display:block;font-weight:600;margin:14px 0 6px}
input[type=text],input[type=email],input[type=password]{width:100%;padding:10px 12px;border:1px solid var(--line);border-radius:8px;background:var(--bg);color:var(--text);font:inherit}
input:focus{outline:2px solid var(--accent2);outline-offset:1px}
.hint{color:var(--muted);font-size:13px;margin:4px 0 0}
button{margin-top:20px;width:100%;padding:11px;border:0;border-radius:8px;background:linear-gradient(90deg,var(--accent),var(--accent2));color:#fff;font:inherit;font-weight:700;cursor:pointer}
.error{background:var(--danger-bg);color:var(--danger);border-radius:8px;padding:10px 12px;margin:0 0 12px}
.error p{margin:0}.links{margin-top:18px;font-size:14px;color:var(--muted)}.links a{color:var(--accent2)}
.cf-turnstile{margin-top:16px;min-height:65px}"#;

/// What a form page shows.
pub struct Form<'a> {
    pub lang: Lang,
    pub heading: &'a str,
    pub action: &'a str,
    pub hidden: &'a [(&'a str, &'a str)],
    pub errors: &'a [&'a str],
    pub fields: &'a [Field<'a>],
    pub submit: &'a str,
    /// Turnstile site key and action, when the page asks for it.
    pub turnstile: Option<(&'a str, &'a str)>,
    /// Links below the form: (text before, link text, address).
    pub links: &'a [(&'a str, &'a str, &'a str)],
}

pub struct Field<'a> {
    pub name: &'a str,
    pub label: &'a str,
    pub kind: &'a str,
    pub value: &'a str,
    pub hint: Option<&'a str>,
    pub autocomplete: &'a str,
    pub max: usize,
}

fn shell(lang: Lang, body: &str) -> String {
    let t = texts(lang);
    format!(
        "<!doctype html>\n<html lang=\"{lang}\" dir=\"{dir}\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>{title}</title><link rel=\"stylesheet\" href=\"/static/style.css\"></head>\
         <body><main>{body}</main></body></html>",
        lang = lang.tag(),
        dir = lang.dir(),
        title = esc(t.title),
    )
}

pub fn form(form: &Form<'_>) -> Response {
    let mut body = format!(
        "<div class=\"card\"><h1>{}</h1><p class=\"sub\">{}</p>",
        esc(form.heading),
        esc(texts(form.lang).title)
    );
    if !form.errors.is_empty() {
        body.push_str("<div class=\"error\" role=\"alert\">");
        for error in form.errors {
            body.push_str(&format!("<p>{}</p>", esc(error)));
        }
        body.push_str("</div>");
    }
    body.push_str(&format!(
        "<form method=\"post\" action=\"{}\" autocomplete=\"on\">",
        esc(form.action)
    ));
    for (name, value) in form.hidden {
        body.push_str(&format!(
            "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
            esc(name),
            esc(value)
        ));
    }
    for field in form.fields {
        body.push_str(&format!(
            "<label for=\"{name}\">{label}</label><input id=\"{name}\" name=\"{name}\" type=\"{kind}\" \
             value=\"{value}\" maxlength=\"{max}\" autocomplete=\"{auto}\" required spellcheck=\"false\" autocapitalize=\"off\">",
            name = esc(field.name),
            label = esc(field.label),
            kind = esc(field.kind),
            value = esc(field.value),
            max = field.max,
            auto = esc(field.autocomplete),
        ));
        if let Some(hint) = field.hint {
            body.push_str(&format!("<p class=\"hint\">{}</p>", esc(hint)));
        }
    }
    if let Some((key, action)) = form.turnstile {
        body.push_str(&format!(
            "<div class=\"cf-turnstile\" data-sitekey=\"{}\" data-action=\"{}\" data-language=\"{}\"></div>",
            esc(key),
            esc(action),
            form.lang.tag()
        ));
    }
    body.push_str(&format!(
        "<button type=\"submit\">{}</button></form>",
        esc(form.submit)
    ));
    if !form.links.is_empty() {
        body.push_str("<div class=\"links\">");
        for (before, text, href) in form.links {
            body.push_str(&format!(
                "<p>{} <a href=\"{}\">{}</a></p>",
                esc(before),
                esc(href),
                esc(text)
            ));
        }
        body.push_str("</div>");
    }
    body.push_str("</div>");
    if form.turnstile.is_some() {
        body.push_str(&format!(
            "<script src=\"{TURNSTILE_SCRIPT}\" async defer></script>"
        ));
    }
    headers(Html(shell(form.lang, &body)).into_response())
}

/// A page with a heading and a sentence.
pub fn message(lang: Lang, heading: &str, text: &str) -> Response {
    let body = format!(
        "<div class=\"card\"><h1>{}</h1><p>{}</p></div>",
        esc(heading),
        esc(text)
    );
    headers(Html(shell(lang, &body)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_cannot_leave_their_place_in_a_page() {
        assert_eq!(
            esc(r#"<script>alert("x")</script>'&"#),
            "&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt;&#39;&amp;"
        );
        let hostile = "\"><img src=x onerror=alert(1)><input value=\"";
        let page = form(&Form {
            lang: Lang::Ru,
            heading: "h",
            action: "/login",
            hidden: &[("rid", hostile)],
            errors: &[hostile],
            fields: &[Field {
                name: "login",
                label: "l",
                kind: "text",
                value: hostile,
                hint: None,
                autocomplete: "username",
                max: 254,
            }],
            submit: "s",
            turnstile: Some(("key\"x", "login")),
            links: &[],
        });
        let csp = page.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(csp.contains("frame-ancestors 'none'") && !csp.contains("unsafe-inline"));
        assert_eq!(page.headers()[header::X_FRAME_OPTIONS], "DENY");
        let body = futures_body(page);
        assert!(!body.contains("<img"), "{body}");
        assert!(body.contains("&quot;&gt;&lt;img src=x onerror=alert(1)&gt;"));
        assert!(body.contains("data-sitekey=\"key&quot;x\""));
        // One script only: Turnstile's.
        assert_eq!(body.matches("<script").count(), 1);
        assert!(body.contains(&format!("<script src=\"{TURNSTILE_SCRIPT}\"")));
        assert!(body.contains("lang=\"ru\""));
        assert!(message(Lang::Ar, "x", "y").status().is_success());
        assert_eq!(Lang::parse("ar-EG"), Lang::Ar);
        assert_eq!(Lang::parse("<x"), Lang::En);
    }

    fn futures_body(response: Response) -> String {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
            use http_body_util::BodyExt;
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            String::from_utf8(bytes.to_vec()).unwrap()
        })
    }
}
