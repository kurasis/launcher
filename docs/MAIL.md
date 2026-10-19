# Letters from the account service (SMTP)

The account service at oauth.lapi.sh sends letters: email confirmation, password reset,
"your password was changed", "your email was changed" (to the old address) and "your
account was deleted". Until an SMTP server is set, letters wait in its outbox (sealed,
three days at most), nobody can confirm an address or reset a password by mail, and
friends does not ask for a confirmed address.

## What to set

In the repository on GitHub: **Settings → Secrets and variables → Actions → New
repository secret**. Two secrets:

| Secret      | Value                                                                           |
| ----------- | ------------------------------------------------------------------------------- |
| `SMTP_URL`  | `smtps://USER:PASSWORD@HOST:465` (TLS from the start), or                       |
|             | `smtp://USER:PASSWORD@HOST:587?tls=required` (STARTTLS)                         |
| `MAIL_FROM` | The sender as letters show it, for example `Sferum Launcher <no-reply@lapi.sh>` |

- `USER`, `PASSWORD` and `HOST` come from the mail provider (any provider with SMTP
  access: Amazon SES, Postmark, Mailgun, Zoho, Yandex 360 and others).
- Characters in the user or password that have a meaning in an address are written
  percent-encoded: `@` as `%40`, `:` as `%3A`, `/` as `%2F`, `?` as `%3F`, `#` as `%23`,
  `%` as `%25`. For example the password `p@ss:word` becomes `p%40ss%3Aword`.
- The address in `MAIL_FROM` must be one the provider lets this account send from.

The secrets never appear in a log: the "Account service" workflow passes them to the
server on standard input, and the server keeps them in
`/home/www/lapi/oauth/secrets.env` (readable by the service alone). Only their names are
printed.

## DNS, so letters are not taken for spam

At the DNS of `lapi.sh` (Cloudflare → DNS), add what the provider gives:

- **SPF**: a `TXT` record on the sending domain, for example
  `v=spf1 include:<provider's domain> ~all`;
- **DKIM**: one or more `TXT` or `CNAME` records with the provider's keys;
- **DMARC**: a `TXT` record on `_dmarc.lapi.sh`, for example
  `v=DMARC1; p=quarantine; rua=mailto:<your address>`.

## Putting it to work

1. Add both secrets.
2. Deploy the account service: **Actions → Account service → Run workflow** on `main`
   with task `deploy` (any later change to the service on `main` deploys it too). Each
   deploy writes the secrets again, so changing a secret means one more deploy.
3. Check:
   - the deploy step lists `LAPI_OAUTH_SMTP_URL LAPI_OAUTH_MAIL_FROM` among the
     secrets;
   - on the server, `journalctl -u lapi-oauth` no longer says "no SMTP server";
   - a test registration gets its confirmation letter (look in spam too).

What changes then:

- Letters waiting in the outbox (up to three days old) go out.
- Within 15 seconds friends starts asking for a confirmed address
  (`LAPI_FRIENDS_REQUIRE_EMAIL=auto`, the default). Players with an unconfirmed
  address see "Friends and chats need a confirmed email" and can send the letter again
  from Accounts. To keep friends open to unconfirmed addresses anyway, set
  `LAPI_FRIENDS_REQUIRE_EMAIL=0` in `/home/www/lapi/friends/friends.env` (as root) and
  run `systemctl restart lapi-friends`; `1` asks for it always.

A failed letter is tried again after 20 s, 40 s, … up to an hour, for three days;
`journalctl -u lapi-oauth` shows the SMTP server's answer (never the recipient).
