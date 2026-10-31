//! The two `SimpleDateFormat`s of the ban lists, on the JVM's default time zone (they are static
//! `SimpleDateFormat`s, made once): `BanListEntry.DATE_FORMAT` `yyyy-MM-dd HH:mm:ss Z` for the files
//! and `PlayerList.BAN_DATE_FORMAT` `yyyy-MM-dd 'at' HH:mm:ss z` for the kick message.
//!
//! Checked against the real class (`LeafUserListsOracle.java`, rows `date_format`, `date_parse`).
//! Not modelled: the Julian calendar before 1582 and years outside jiff's range (-9999..=9999).

use jiff::Timestamp;
use jiff::tz::TimeZone;

/// `days_from_civil`: days since 1970-01-01 of a proleptic Gregorian date (month 1..=12).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The local fields of an instant in `zone`: date, time, offset in minutes and the zone's short name.
struct Local {
    year: i32,
    month: i8,
    day: i8,
    hour: i8,
    minute: i8,
    second: i8,
    offset_seconds: i32,
    /// The zone is one of the UTC names, which keep their name where the others are `GMT+hh:mm`.
    utc_named: bool,
}

fn local(millis: i64, zone: &TimeZone) -> Option<Local> {
    let zoned = Timestamp::from_millisecond(millis)
        .ok()?
        .to_zoned(zone.clone());
    Some(Local {
        year: i32::from(zoned.year()),
        month: zoned.month(),
        day: zoned.day(),
        hour: zoned.hour(),
        minute: zoned.minute(),
        second: zoned.second(),
        offset_seconds: zoned.offset().seconds(),
        utc_named: matches!(
            zone.iana_name(),
            Some(
                "UTC"
                    | "Etc/UTC"
                    | "Etc/UCT"
                    | "UCT"
                    | "Etc/Universal"
                    | "Universal"
                    | "Etc/Zulu"
                    | "Zulu"
            )
        ),
    })
}

/// `DATE_FORMAT.format(new Date(millis))`.
pub fn format_date(millis: i64, zone: &TimeZone) -> String {
    let Some(l) = local(millis, zone) else {
        return String::new();
    };
    let offset = l.offset_seconds.abs() / 60;
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}{:02}{:02}",
        l.year,
        l.month,
        l.day,
        l.hour,
        l.minute,
        l.second,
        if l.offset_seconds < 0 { '-' } else { '+' },
        offset / 60,
        offset % 60,
    )
}

/// `BAN_DATE_FORMAT.format(new Date(millis))`.
pub fn format_login_date(millis: i64, zone: &TimeZone) -> String {
    let Some(l) = local(millis, zone) else {
        return String::new();
    };
    // `z` in `Locale.ROOT`, which has no zone names: `UTC` for the UTC zones, else `GMT`, followed
    // by the offset when there is one. (Java derives it from the zone's *current* raw offset plus the
    // daylight saving at that time, so an instant long ago in a zone whose rules changed can differ.)
    let offset = l.offset_seconds.abs() / 60;
    let name = if l.utc_named {
        "UTC".to_owned()
    } else if offset == 0 {
        "GMT".to_owned()
    } else {
        format!(
            "GMT{}{:02}:{:02}",
            if l.offset_seconds < 0 { '-' } else { '+' },
            offset / 60,
            offset % 60
        )
    };
    format!(
        "{:04}-{:02}-{:02} at {:02}:{:02}:{:02} {name}",
        l.year, l.month, l.day, l.hour, l.minute, l.second,
    )
}

/// `DATE_FORMAT.parse(text).getTime()`: `None` where it throws `ParseException`. `SimpleDateFormat`
/// skips spaces and tabs before each field, reads a field's digits (a leading `-` allowed) without a
/// width limit, needs each `-`, space and `:` of the pattern exactly, ignores whatever follows the
/// zone, and is lenient: a month 13 is January of the next year, an hour 25 is 1 am the next day.
pub fn parse_date(text: &str) -> Option<i64> {
    let mut chars = Scanner {
        text: text.as_bytes(),
        at: 0,
    };
    let year = chars.number()?;
    chars.literal(b'-')?;
    let month = chars.number()?;
    chars.literal(b'-')?;
    let day = chars.number()?;
    chars.literal(b' ')?;
    let hour = chars.number()?;
    chars.literal(b':')?;
    let minute = chars.number()?;
    chars.literal(b':')?;
    let second = chars.number()?;
    chars.literal(b' ')?;
    let offset_minutes = chars.zone()?;
    let months = year.checked_mul(12)?.checked_add(month - 1)?;
    let days = days_from_civil(months.div_euclid(12), months.rem_euclid(12) + 1, 1)
        .checked_add(day - 1)?;
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(hour.checked_mul(3600)?)?
        .checked_add(minute.checked_mul(60)?)?
        .checked_add(second)?
        .checked_sub(offset_minutes * 60)?;
    seconds.checked_mul(1000)
}

struct Scanner<'a> {
    text: &'a [u8],
    at: usize,
}

impl Scanner<'_> {
    fn skip_blanks(&mut self) {
        while matches!(self.text.get(self.at), Some(b' ' | b'\t')) {
            self.at += 1;
        }
    }

    /// A pattern character other than a field: matched exactly.
    fn literal(&mut self, c: u8) -> Option<()> {
        (self.text.get(self.at) == Some(&c)).then(|| self.at += 1)
    }

    /// A numeric field: blanks, an optional `-`, then ASCII digits (at least one).
    fn number(&mut self) -> Option<i64> {
        self.skip_blanks();
        let negative = self.text.get(self.at) == Some(&b'-');
        let start = self.at + usize::from(negative);
        let digits = self.text[start.min(self.text.len())..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits == 0 {
            return None;
        }
        self.at = start + digits;
        let value: i64 = std::str::from_utf8(&self.text[start..self.at])
            .ok()?
            .parse()
            .ok()?;
        Some(if negative { -value } else { value })
    }

    /// The `Z` field: `+hhmm` or `-hhmm` (RFC 822: two digit hours up to 23, two digit minutes up to
    /// 59), `GMT` or `UTC` alone or followed by a sign and `h[h][:mm]`. Minutes east of UTC.
    fn zone(&mut self) -> Option<i64> {
        self.skip_blanks();
        let rest = &self.text[self.at.min(self.text.len())..];
        let sign = match rest.first()? {
            b'+' => 1,
            b'-' => -1,
            _ => {
                let word = rest.get(..3)?;
                if !(word.eq_ignore_ascii_case(b"GMT") || word.eq_ignore_ascii_case(b"UTC")) {
                    return None;
                }
                self.at += 3;
                // `GMT+h:mm`: a custom zone; anything else after it is ignored.
                return Some(self.custom_offset().unwrap_or(0));
            }
        };
        let digit = |i: usize| {
            rest.get(i)
                .filter(|b| b.is_ascii_digit())
                .map(|b| i64::from(b - b'0'))
        };
        let hours = digit(1)? * 10 + digit(2)?;
        let minutes = digit(3)? * 10 + digit(4)?;
        if hours > 23 || minutes > 59 {
            return None;
        }
        self.at += 5;
        Some(sign * (hours * 60 + minutes))
    }

    /// After `GMT`: `[+-]h[h]:mm` (the colon form of `subParseNumericZone`), if present.
    fn custom_offset(&mut self) -> Option<i64> {
        let rest = &self.text[self.at.min(self.text.len())..];
        let sign = match rest.first()? {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let digit = |i: usize| {
            rest.get(i)
                .filter(|b| b.is_ascii_digit())
                .map(|b| i64::from(b - b'0'))
        };
        let mut i = 1;
        let mut hours = digit(i)?;
        i += 1;
        if let Some(d) = digit(i) {
            hours = hours * 10 + d;
            i += 1;
        }
        if hours > 23 || rest.get(i) != Some(&b':') {
            return None;
        }
        let minutes = digit(i + 1)? * 10 + digit(i + 2)?;
        if minutes > 59 {
            return None;
        }
        self.at += i + 3;
        Some(sign * (hours * 60 + minutes))
    }
}
