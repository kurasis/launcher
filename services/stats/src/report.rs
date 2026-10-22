//! What the commands print: tables for the owner's terminal, or CSV with
//! `--csv`. Days are UTC; weeks start on Monday. Everything goes through
//! `plain` before it reaches the terminal.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use chrono::{Datelike, Duration, NaiveDate};

use crate::ads::Online;
use crate::model::{self, CUTS, Period};
use crate::store::{self, Store};

const BAR: usize = 24;

/// `12,345`.
pub fn number(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// `2.2%`; a dash without a whole.
pub fn share(part: i64, whole: i64) -> String {
    if whole <= 0 {
        "-".to_owned()
    } else {
        format!("{:.1}%", part as f64 * 100.0 / whole as f64)
    }
}

fn bar(part: i64, most: i64) -> String {
    if most <= 0 || part <= 0 {
        return String::new();
    }
    let width = ((part as f64 / most as f64) * BAR as f64).round().max(1.0) as usize;
    "█".repeat(width.min(BAR))
}

/// A line of blocks, one per value.
pub fn spark(values: &[i64]) -> String {
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let (Some(low), Some(high)) = (values.iter().min(), values.iter().max()) else {
        return String::new();
    };
    values
        .iter()
        .map(|v| {
            if high == low {
                BLOCKS[0]
            } else {
                BLOCKS[((v - low) * 7 / (high - low)) as usize]
            }
        })
        .collect()
}

/// One CSV field: quoted when needed, and never read as a formula.
fn field(text: &str) -> String {
    let text = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

fn csv_line(fields: &[String]) -> String {
    fields
        .iter()
        .map(|f| field(f))
        .collect::<Vec<_>>()
        .join(",")
}

/// The days of a report: the last `days` up to today.
pub fn last(today: NaiveDate, days: i64) -> (NaiveDate, NaiveDate) {
    (today - Duration::days(days.clamp(1, 3660) - 1), today)
}

fn monday(day: NaiveDate) -> NaiveDate {
    day - Duration::days(i64::from(day.weekday().num_days_from_monday()))
}

fn month_first(day: NaiveDate) -> NaiveDate {
    day.with_day(1).unwrap_or(day)
}

pub fn overview(store: &Store, today: NaiveDate, days: i64) -> Result<String, String> {
    let mut out = String::new();
    let day_total = |day: NaiveDate| store.total(day, day, Period::Day);
    let this_week = monday(today);
    let last_week = this_week - Duration::days(7);
    let this_month = month_first(today);
    let last_month = month_first(this_month - Duration::days(1));
    let (from, to) = last(today, days);
    let daily: Vec<i64> = store.daily(from, to, Period::Day)?.into_values().collect();
    let _ = writeln!(
        out,
        "Players (UTC days; launchers with statistics on, no IDs)"
    );
    let rows = [
        (format!("today {today} (so far)"), day_total(today)?),
        (
            "yesterday".to_owned(),
            day_total(today - Duration::days(1))?,
        ),
        (
            "this week (since Monday)".to_owned(),
            store.total(this_week, today, Period::Week)?,
        ),
        (
            "last week".to_owned(),
            store.total(last_week, this_week - Duration::days(1), Period::Week)?,
        ),
        (
            "this month".to_owned(),
            store.total(this_month, today, Period::Month)?,
        ),
        (
            "last month".to_owned(),
            store.total(last_month, this_month - Duration::days(1), Period::Month)?,
        ),
        (
            "new in the last 7 days".to_owned(),
            store.total(today - Duration::days(6), today, Period::New)?,
        ),
    ];
    for (label, n) in rows {
        let _ = writeln!(out, "  {label:<28} {:>10}", number(n));
    }
    if let (Some(low), Some(high)) = (daily.iter().min(), daily.iter().max()) {
        let _ = writeln!(
            out,
            "  per day, last {} days      {}  {} to {}",
            daily.len(),
            spark(&daily),
            number(*low),
            number(*high)
        );
    }
    // The newest install week whose fourth week after is over.
    let cohort = model::week_of(this_week - Duration::days(35));
    let start = model::week_start(&cohort).unwrap_or(today);
    let size = store.cohort(&cohort, None, Period::New, start, start + Duration::days(6))?;
    let fourth = start + Duration::days(28);
    let kept = store.cohort(
        &cohort,
        None,
        Period::Week,
        fourth,
        fourth + Duration::days(6),
    )?;
    let _ = writeln!(
        out,
        "  {:<28} {:>10}  (installed in {cohort}: {} of {})",
        "still there after 4 weeks",
        share(kept, size),
        number(kept),
        number(size)
    );
    let _ = writeln!(out);
    let countries = store.values(this_month, today, Period::Month, "country")?;
    let _ = writeln!(out, "Countries this month");
    out.push_str(&shares(&countries, 12));
    let dropped = store.dropped(from, to)?;
    if !dropped.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "Left out, last {days} days");
        let line: Vec<String> = dropped
            .iter()
            .map(|(reason, n)| format!("{reason} {}", number(*n)))
            .collect();
        let _ = writeln!(out, "  {}", line.join(" · "));
    }
    Ok(out)
}

/// Values with their share and a bar, the first `limit`, then the rest.
fn shares(values: &[(String, i64)], limit: usize) -> String {
    let mut out = String::new();
    let whole: i64 = values.iter().map(|(_, n)| n).sum();
    let most = values.first().map_or(0, |(_, n)| *n);
    let width = values
        .iter()
        .take(limit)
        .map(|(v, _)| v.chars().count())
        .max()
        .unwrap_or(0)
        .clamp(6, 40);
    for (value, n) in values.iter().take(limit) {
        let _ = writeln!(
            out,
            "  {value:<width$}  {:>6}  {:<BAR$}  {}",
            share(*n, whole),
            bar(*n, most),
            number(*n)
        );
    }
    if values.len() > limit {
        let rest: i64 = values.iter().skip(limit).map(|(_, n)| n).sum();
        let label = format!("others ({})", values.len() - limit);
        let _ = writeln!(
            out,
            "  {label:<width$}  {:>6}  {:<BAR$}  {}",
            share(rest, whole),
            bar(rest, most),
            number(rest)
        );
    }
    if values.is_empty() {
        let _ = writeln!(out, "  nothing yet");
    }
    out
}

pub fn players(store: &Store, today: NaiveDate, days: i64, csv: bool) -> Result<String, String> {
    let (from, to) = last(today, days);
    let mut columns = Vec::new();
    for period in Period::ALL {
        columns.push(store.daily(from, to, period)?);
    }
    let mut out = String::new();
    if csv {
        let _ = writeln!(out, "day,players,first_of_week,first_of_month,new");
    } else {
        let _ = writeln!(
            out,
            "day           players  first of week  first of month       new"
        );
    }
    let mut day = to;
    while day >= from {
        let n = |i: usize| columns[i].get(&day).copied().unwrap_or(0);
        if csv {
            let _ = writeln!(
                out,
                "{}",
                csv_line(&[
                    day.to_string(),
                    n(0).to_string(),
                    n(1).to_string(),
                    n(2).to_string(),
                    n(3).to_string()
                ])
            );
        } else {
            let _ = writeln!(
                out,
                "{day}  {:>9}  {:>13}  {:>14}  {:>8}",
                number(n(0)),
                number(n(1)),
                number(n(2)),
                number(n(3))
            );
        }
        day -= Duration::days(1);
    }
    Ok(out)
}

/// The cut a command names: its own name or a plural.
pub fn cut_name(name: &str) -> Option<&'static str> {
    let name = match name {
        "countries" => "country",
        "systems" => "system",
        "versions" => "version",
        "languages" => "language",
        "timezones" | "tz" => "country_tz",
        "cohorts" => "cohort",
        other => other,
    };
    CUTS.iter().find(|c| **c == name).copied()
}

pub fn period_name(name: &str) -> Option<Period> {
    match name {
        "day" | "d" => Some(Period::Day),
        "week" | "w" => Some(Period::Week),
        "month" | "m" => Some(Period::Month),
        "new" | "n" => Some(Period::New),
        _ => None,
    }
}

pub fn cut(
    store: &Store,
    today: NaiveDate,
    (cut, period): (&str, Period),
    days: i64,
    limit: usize,
    csv: bool,
) -> Result<String, String> {
    let (from, to) = last(today, days);
    let values = store.values(from, to, period, cut)?;
    let mut out = String::new();
    if csv {
        let _ = writeln!(out, "value,count");
        for (value, n) in &values {
            let _ = writeln!(out, "{}", csv_line(&[value.clone(), n.to_string()]));
        }
        return Ok(out);
    }
    let what = match period {
        Period::Day => "players per day, summed",
        Period::Week => "first pings of a week, summed",
        Period::Month => "first pings of a month, summed",
        Period::New => "new installs",
    };
    let _ = writeln!(out, "{cut}, {from} to {to} ({what})");
    out.push_str(&shares(&values, limit));
    Ok(out)
}

pub fn retention(
    store: &Store,
    today: NaiveDate,
    weeks: i64,
    country: Option<&str>,
    csv: bool,
) -> Result<String, String> {
    const AFTER: [i64; 5] = [1, 2, 4, 8, 12];
    let this_week = monday(today);
    let mut out = String::new();
    if csv {
        let _ = writeln!(
            out,
            "installed,launchers,week_1,week_2,week_4,week_8,week_12"
        );
    } else {
        let _ = writeln!(
            out,
            "Launchers still there k weeks after their install week{}",
            country.map(|c| format!(" ({c})")).unwrap_or_default()
        );
        let _ = writeln!(
            out,
            "{:<10}{:>10}{:>9}{:>9}{:>9}{:>9}{:>9}",
            "installed", "launchers", "week 1", "week 2", "week 4", "week 8", "week 12"
        );
    }
    for back in (1..=weeks.clamp(1, 104)).rev() {
        let start = this_week - Duration::days(7 * back);
        let week = model::week_of(start);
        let size = store.cohort(
            &week,
            country,
            Period::New,
            start,
            start + Duration::days(6),
        )?;
        let mut cells = Vec::new();
        for k in AFTER {
            let from = start + Duration::days(7 * k);
            // Only weeks that are over.
            if from >= this_week {
                cells.push(None);
                continue;
            }
            let kept =
                store.cohort(&week, country, Period::Week, from, from + Duration::days(6))?;
            cells.push(Some(kept));
        }
        if csv {
            let mut fields = vec![week.clone(), size.to_string()];
            fields.extend(
                cells
                    .iter()
                    .map(|c| c.map(|n| n.to_string()).unwrap_or_default()),
            );
            let _ = writeln!(out, "{}", csv_line(&fields));
        } else {
            let cells: String = cells
                .iter()
                .map(|c| format!("{:>9}", c.map_or("-".to_owned(), |n| share(n, size))))
                .collect();
            let _ = writeln!(out, "{week:<10}{:>10}{cells}", number(size));
        }
    }
    Ok(out)
}

/// Events of the `ads` table, as the funnel goes.
const EVENTS: [&str; 23] = [
    "shown",
    "reach_day",
    "reach_ever",
    "menu",
    "version_list",
    "copy",
    "report",
    "play",
    "license",
    "installed",
    "install_failed",
    "started",
    "joined",
    "refused_version",
    "refused_full",
    "refused_whitelist",
    "refused_ban",
    "refused_noanswer",
    "refused_other",
    "played",
    "minutes",
    "back_day",
    "back_week",
];

pub fn ads(store: &Store, today: NaiveDate, days: i64, csv: bool) -> Result<String, String> {
    let (from, to) = last(today, days);
    let totals = store.ad_totals(from, to, None)?;
    let names = store.ad_names(from - Duration::days(1), to)?;
    let mut by_ad: BTreeMap<i64, BTreeMap<String, i64>> = BTreeMap::new();
    for ((ad, event), n) in totals {
        by_ad.entry(ad).or_default().insert(event, n);
    }
    let mut out = String::new();
    if csv {
        let mut header = vec!["ad".to_owned(), "name".to_owned()];
        header.extend(EVENTS.iter().map(|e| (*e).to_owned()));
        let _ = writeln!(out, "{}", csv_line(&header));
    } else {
        let _ = writeln!(out, "Ads, {from} to {to}");
        let _ = writeln!(
            out,
            "   ad  name                                         shown    reach   play    CTR  joined  10 min"
        );
    }
    for (ad, events) in &by_ad {
        let n = |event: &str| events.get(event).copied().unwrap_or(0);
        let name = names.get(ad).cloned().unwrap_or_default();
        if csv {
            let mut fields = vec![ad.to_string(), name];
            fields.extend(EVENTS.iter().map(|e| n(e).to_string()));
            let _ = writeln!(out, "{}", csv_line(&fields));
        } else {
            let name: String = name.chars().take(40).collect();
            let _ = writeln!(
                out,
                "{ad:>5}  {name:<40}  {:>8} {:>8} {:>6} {:>6} {:>7} {:>7}",
                number(n("shown")),
                number(n("reach_day")),
                number(n("play")),
                share(n("play"), n("shown")),
                number(n("joined")),
                number(n("played"))
            );
        }
    }
    if by_ad.is_empty() && !csv {
        let _ = writeln!(out, "  no counts yet");
    }
    Ok(out)
}

/// Average players online, of the hours that answered between `from` and
/// `to` (unix seconds).
fn average(online: &Online, from: i64, to: i64) -> Option<i64> {
    let hours: Vec<i64> = online
        .hours
        .iter()
        .filter(|(at, _)| *at >= from && *at < to)
        .filter_map(|(_, n)| *n)
        .collect();
    (!hours.is_empty()).then(|| hours.iter().sum::<i64>() / hours.len() as i64)
}

pub fn ad(
    store: &Store,
    today: NaiveDate,
    (ad, days): (i64, i64),
    online: Result<Online, String>,
    csv: bool,
) -> Result<String, String> {
    let (from, to) = last(today, days);
    let daily = store.ad_days(from, to, ad)?;
    let mut out = String::new();
    if csv {
        let mut header = vec!["day".to_owned()];
        header.extend(EVENTS.iter().map(|e| (*e).to_owned()));
        let _ = writeln!(out, "{}", csv_line(&header));
        for (day, events) in &daily {
            let mut fields = vec![day.clone()];
            fields.extend(
                EVENTS
                    .iter()
                    .map(|e| events.get(*e).copied().unwrap_or(0).to_string()),
            );
            let _ = writeln!(out, "{}", csv_line(&fields));
        }
        return Ok(out);
    }
    let totals: BTreeMap<String, i64> = store
        .ad_totals(from, to, Some(ad))?
        .into_iter()
        .map(|((_, event), n)| (event, n))
        .collect();
    let n = |event: &str| totals.get(event).copied().unwrap_or(0);
    let name = store
        .ad_name(ad)?
        .unwrap_or_else(|| "(not seen)".to_owned());
    let _ = writeln!(out, "{name} · ad {ad} · {from} to {to}");
    let reach = n("reach_day");
    let _ = writeln!(
        out,
        "Shown          {:>9}   reach {} launcher-days, {} new launchers; {} per launcher-day",
        number(n("shown")),
        number(reach),
        number(n("reach_ever")),
        if reach > 0 {
            format!("{:.1}", n("shown") as f64 / reach as f64)
        } else {
            "-".to_owned()
        }
    );
    let _ = writeln!(
        out,
        "Play           {:>9}   {} of the shown",
        number(n("play")),
        share(n("play"), n("shown"))
    );
    match online {
        Ok(online) => {
            let now = chrono::Utc::now().timestamp();
            let during = average(&online, online.starts, online.ends.min(now));
            let before = average(&online, online.starts - 7 * 86_400, online.starts);
            let _ = writeln!(
                out,
                "Online         {:>9}   on average while it shows; {} in the week before",
                during.map_or("-".to_owned(), number),
                before.map_or("-".to_owned(), number)
            );
        }
        Err(e) => {
            let _ = writeln!(out, "Online         {e}");
        }
    }
    let _ = writeln!(out, "From Play");
    let play = n("play");
    for (label, event) in [
        ("pressed Play", "play"),
        ("started the game", "started"),
        ("joined the server", "joined"),
        ("played 10 minutes", "played"),
        ("back a day later", "back_day"),
        ("back a week later", "back_week"),
    ] {
        let _ = writeln!(
            out,
            "  {label:<20} {:>8}  {:>6}  {}",
            number(n(event)),
            share(n(event), play),
            bar(n(event), play)
        );
    }
    let _ = writeln!(
        out,
        "Stopped before the game: needs a Microsoft account {}, installed first {}, install failed {}",
        number(n("license")),
        number(n("installed")),
        number(n("install_failed"))
    );
    let refused: Vec<String> = model::REFUSALS
        .iter()
        .map(|r| format!("{r} {}", number(n(model::refused_event(r)))))
        .collect();
    let _ = writeln!(out, "Turned away: {}", refused.join(", "));
    let joined = n("joined");
    let _ = writeln!(
        out,
        "Minutes on the server {} ({} per player who joined)",
        number(n("minutes")),
        if joined > 0 {
            number(n("minutes") / joined)
        } else {
            "-".to_owned()
        }
    );
    let _ = writeln!(
        out,
        "Menu {}, version list {}, address copied {}, reported {}",
        number(n("menu")),
        number(n("version_list")),
        number(n("copy")),
        number(n("report"))
    );
    for (title, event, cut) in [
        ("Versions started", "picked", "version"),
        ("Countries (shown)", "shown", "country"),
        ("Languages (shown)", "shown", "ui"),
    ] {
        let values = store.ad_values(from, to, ad, event, cut)?;
        let whole: i64 = values.iter().map(|(_, n)| n).sum();
        let mut parts: Vec<String> = values
            .iter()
            .take(8)
            .map(|(v, n)| format!("{v} {}", share(*n, whole)))
            .collect();
        if values.len() > 8 {
            let rest: i64 = values.iter().skip(8).map(|(_, n)| n).sum();
            parts.push(format!("others {}", share(rest, whole)));
        }
        let parts = if parts.is_empty() {
            "-".to_owned()
        } else {
            parts.join(", ")
        };
        let _ = writeln!(out, "{title}: {parts}");
    }
    let _ = writeln!(out, "Per day");
    let _ = writeln!(
        out,
        "  day             shown    reach    play  joined  minutes"
    );
    for (day, events) in daily.iter().rev() {
        let n = |event: &str| events.get(event).copied().unwrap_or(0);
        let _ = writeln!(
            out,
            "  {day}  {:>9} {:>8} {:>7} {:>7} {:>8}",
            number(n("shown")),
            number(n("reach_day")),
            number(n("play")),
            number(n("joined")),
            number(n("minutes"))
        );
    }
    Ok(out)
}

pub fn ord(store: &Store, month: &str, csv: bool) -> Result<String, String> {
    let start = store::month_start(month).ok_or("give the month as YYYY-MM")?;
    let month = start.format("%Y-%m").to_string();
    let erids = store.by_erid(&month)?;
    let ads = store.erid_ads(&month)?;
    let mut out = String::new();
    if csv {
        let _ = writeln!(out, "erid,ads,shown,shown_in_russia");
        for (erid, (_, shown, russia)) in &erids {
            let ids: Vec<String> = ads
                .get(erid)
                .map(|ids| ids.iter().map(i64::to_string).collect())
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "{}",
                csv_line(&[
                    erid.clone(),
                    ids.join(" "),
                    shown.to_string(),
                    russia.to_string()
                ])
            );
        }
        return Ok(out);
    }
    let _ = writeln!(
        out,
        "Impressions per erid, {month} (UTC days, {} to {})",
        start,
        store::month_end(start)
    );
    let width = erids.keys().map(String::len).max().unwrap_or(0).max(9);
    let _ = writeln!(
        out,
        "  {:<width$}  {:>4}  {:>9}  {:>9}",
        "erid", "ads", "shown", "in Russia"
    );
    for (erid, (count, shown, russia)) in &erids {
        let label = if erid.is_empty() { "(no erid)" } else { erid };
        let _ = writeln!(
            out,
            "  {label:<width$}  {count:>4}  {:>9}  {:>9}",
            number(*shown),
            number(*russia)
        );
        if let Some(ids) = ads.get(erid) {
            let ids: Vec<String> = ids.iter().map(i64::to_string).collect();
            let _ = writeln!(out, "    ads {}", ids.join(", "));
        }
    }
    if erids.is_empty() {
        let _ = writeln!(out, "  no impressions counted");
    }
    let _ = writeln!(
        out,
        "Each erid's impressions go to the ad register (ERIR) through the ORD by the 30th of the next month."
    );
    Ok(out)
}
