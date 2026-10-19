//! What each letter says, in the account's language, and how it looks: a plain
//! text part and an HTML part made from the same words. The HTML is built for
//! mail clients (tables, inline styles, no scripts, no images to load), and
//! every word that comes from an account (nick, address) or a link is escaped.

use crate::pages::Lang;

/// The words of one letter.
pub struct Content {
    pub subject: String,
    /// The line mail clients show after the subject.
    pub preheader: String,
    pub heading: String,
    pub greeting: String,
    pub paragraphs: Vec<String>,
    /// The main action: its label and link.
    pub button: Option<(String, String)>,
    /// A small line under the button, such as how long the link works.
    pub after_button: Option<String>,
    /// "If this was not you": shown in a highlighted box.
    pub warning: Option<String>,
}

pub struct Rendered {
    pub subject: String,
    pub text: String,
    pub html: String,
}

struct Words {
    copy_link: &'static str,
    footer: &'static str,
}

fn words(lang: Lang) -> Words {
    match lang {
        Lang::Ru => Words {
            copy_link: "Если кнопка не открывается, скопируйте ссылку в браузер:",
            footer: "Sferum Launcher · Письмо отправлено автоматически, отвечать на него не нужно.",
        },
        Lang::Ar => Words {
            copy_link: "إذا لم يعمل الزر، فانسخ هذا الرابط إلى المتصفح:",
            footer: "Sferum Launcher · أُرسلت هذه الرسالة تلقائيًا، فلا حاجة إلى الرد عليها.",
        },
        Lang::En => Words {
            copy_link: "If the button does not open, copy this link into your browser:",
            footer: "Sferum Launcher · This letter was sent automatically; there is no need to reply.",
        },
    }
}

/// Text that may come from an account or a link, made safe for HTML text and
/// attribute values.
pub fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

const FONT: &str = "'Segoe UI',Roboto,'Helvetica Neue',Helvetica,Arial,sans-serif";
const ACCENT: &str = "#0f7f5f";
const ACCENT_GRADIENT: &str = "linear-gradient(90deg,#14936c,#0d6b8c)";
const TEXT: &str = "#14212b";
const MUTED: &str = "#5d6d78";
const PAGE: &str = "#eef3f6";
const LINE: &str = "#d3dde3";

impl Content {
    pub fn render(&self, lang: Lang) -> Rendered {
        Rendered {
            subject: self.subject.clone(),
            text: self.text(lang),
            html: self.html(lang),
        }
    }

    fn text(&self, lang: Lang) -> String {
        let mut out = format!("{}\n\n", self.greeting);
        for paragraph in &self.paragraphs {
            out.push_str(paragraph);
            out.push_str("\n\n");
        }
        if let Some((_, link)) = &self.button {
            out.push_str(link);
            out.push_str("\n\n");
        }
        if let Some(after) = &self.after_button {
            out.push_str(after);
            out.push_str("\n\n");
        }
        if let Some(warning) = &self.warning {
            out.push_str(warning);
            out.push_str("\n\n");
        }
        out.push_str("-- \n");
        out.push_str(words(lang).footer);
        out.push('\n');
        out
    }

    fn html(&self, lang: Lang) -> String {
        let w = words(lang);
        let rtl = lang == Lang::Ar;
        let (dir, start, gap) = if rtl {
            ("rtl", "right", "padding-right:12px;")
        } else {
            ("ltr", "left", "padding-left:12px;")
        };
        let p = |text: &str, style: &str| {
            format!(r#"<p style="margin:0 0 14px;{style}">{}</p>"#, esc(text))
        };
        let mut body = String::new();
        body.push_str(&format!(
            r#"<h1 style="margin:0 0 18px;font:700 24px/1.3 {FONT};color:{TEXT};">{}</h1>"#,
            esc(&self.heading)
        ));
        body.push_str(&p(&self.greeting, ""));
        for paragraph in &self.paragraphs {
            body.push_str(&p(paragraph, ""));
        }
        if let Some((label, link)) = &self.button {
            let link = esc(link);
            body.push_str(&format!(
                r#"<table role="presentation" cellpadding="0" cellspacing="0" border="0" style="margin:26px 0 24px;"><tr><td style="border-radius:10px;background:{ACCENT};background-image:{ACCENT_GRADIENT};"><a href="{link}" target="_blank" rel="noopener" style="display:inline-block;padding:14px 30px;font:700 16px/1.2 {FONT};color:#ffffff;text-decoration:none;border-radius:10px;">{}</a></td></tr></table>"#,
                esc(label)
            ));
            if let Some(after) = &self.after_button {
                body.push_str(&p(after, &format!("font-size:14px;color:{MUTED};")));
            }
            body.push_str(&p(
                w.copy_link,
                &format!("margin-bottom:4px;font-size:13px;color:{MUTED};"),
            ));
            body.push_str(&format!(
                r#"<p style="margin:0 0 6px;font-size:13px;line-height:1.5;word-break:break-all;" dir="ltr"><a href="{link}" target="_blank" rel="noopener" style="color:#0d6b8c;">{link}</a></p>"#
            ));
        } else if let Some(after) = &self.after_button {
            body.push_str(&p(after, &format!("font-size:14px;color:{MUTED};")));
        }
        if let Some(warning) = &self.warning {
            body.push_str(&format!(
                r#"<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="margin:22px 0 0;"><tr><td style="padding:14px 16px;border-radius:10px;background:#fff6e0;border:1px solid #f1d9a0;font:14px/1.5 {FONT};color:#5f4600;">{}</td></tr></table>"#,
                esc(warning)
            ));
        }

        format!(
            r#"<!doctype html>
<html lang="{lang}" dir="{dir}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="light">
<meta name="supported-color-schemes" content="light">
<title>{subject}</title>
</head>
<body style="margin:0;padding:0;background:{PAGE};-webkit-text-size-adjust:100%;">
<div style="display:none;max-height:0;max-width:0;overflow:hidden;opacity:0;font-size:1px;line-height:1px;color:{PAGE};">{preheader}</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="background:{PAGE};">
<tr><td align="center" style="padding:32px 16px 24px;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="max-width:560px;">
<tr><td align="{start}" style="padding:0 4px 20px;">
<table role="presentation" cellpadding="0" cellspacing="0" border="0"><tr>
<td width="34" height="34" style="width:34px;height:34px;border-radius:9px;background:{ACCENT};background-image:linear-gradient(135deg,#1aa37a,#0d6b8c);font-size:0;line-height:0;">&nbsp;</td>
<td style="{gap}font:800 20px/1 {FONT};letter-spacing:2px;color:{ACCENT};white-space:nowrap;" dir="ltr">SFERUM&nbsp;<span style="font-weight:600;font-size:12px;letter-spacing:4px;color:{MUTED};">LAUNCHER</span></td>
</tr></table>
</td></tr>
<tr><td style="background:#ffffff;border:1px solid {LINE};border-radius:16px;">
<div style="height:6px;border-radius:16px 16px 0 0;background:{ACCENT};background-image:{ACCENT_GRADIENT};font-size:0;line-height:0;">&nbsp;</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0"><tr>
<td align="{start}" style="padding:30px 34px 34px;font:16px/1.6 {FONT};color:{TEXT};">{body}</td>
</tr></table>
</td></tr>
<tr><td align="center" style="padding:20px 12px 0;font:12px/1.6 {FONT};color:{MUTED};">{footer}</td></tr>
</table>
</td></tr>
</table>
</body>
</html>
"#,
            lang = lang.tag(),
            subject = esc(&self.subject),
            preheader = esc(&self.preheader),
            footer = esc(w.footer),
        )
    }
}
