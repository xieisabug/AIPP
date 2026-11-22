//! 离线农历。低四位为闰哪个月，其余各位按实际月份顺序记录大月。
//! 年表取自 .NET ChineseLunisolarCalendar (1901–2100)，1900 年由 ICU Chinese calendar 补齐。
use chrono::{Datelike, Duration, NaiveDate};

const YEARS: [u32; 201] = [
    0x16d28, 0x7520, 0xea50, 0x164a5, 0x64b0, 0xa9b0, 0x15564, 0x56a0, 0xb590, 0x17522, 0x7520,
    0x1b256, 0xb250, 0xa4b0, 0x14ab5, 0x2ad0, 0x56b0, 0xb692, 0xda90, 0x1d927, 0xe920, 0xd250,
    0x1a4d5, 0xa560, 0x2b60, 0x15b54, 0x6d40, 0xea90, 0x1e922, 0xe920, 0xd266, 0x52b0, 0xa570,
    0x12b65, 0xb5a0, 0x6d40, 0xec93, 0x7490, 0x16937, 0xa930, 0x52b0, 0xa5b6, 0xaad0, 0x56a0,
    0x1b554, 0xba40, 0xb490, 0x1a932, 0xa950, 0x152d7, 0x5360, 0xaad0, 0x15aa5, 0x5b20, 0xda50,
    0x1d4a3, 0xd4a0, 0xa958, 0xa970, 0x5560, 0xab56, 0xad50, 0x6d20, 0xea54, 0xea50, 0x64a0,
    0xc973, 0xa9b0, 0x155a7, 0x56a0, 0xb690, 0x17525, 0xb520, 0xb250, 0x164b4, 0xa4b0, 0x14ab8,
    0x2ad0, 0x56d0, 0xb696, 0xda90, 0xd920, 0x1d254, 0xd250, 0x1a4da, 0xa560, 0x2b60, 0x5b56,
    0x6d50, 0xea90, 0x1e925, 0xe920, 0xd260, 0xa563, 0xa570, 0x14d68, 0x35a0, 0x6d50, 0x16c95,
    0x7490, 0x6930, 0x152b4, 0x52b0, 0xa5b0, 0x155a2, 0x56a0, 0x1b557, 0xba40, 0xb490, 0x1a935,
    0xa950, 0x52d0, 0xaad4, 0xab50, 0x15aa9, 0x5d20, 0xda50, 0x1d4a6, 0xd4a0, 0xc950, 0x152e4,
    0x5560, 0xab50, 0x15b22, 0x6d20, 0xea56, 0x7250, 0x64b0, 0xc975, 0xcab0, 0x55a0, 0xad63,
    0xb690, 0x1752b, 0xb520, 0xb250, 0x1a4b6, 0xa4b0, 0x4ab0, 0x55b5, 0x5ad0, 0xb6a0, 0x1b522,
    0xd920, 0x1d257, 0xd250, 0xa550, 0x14ad5, 0x4b60, 0x5b50, 0xdaa3, 0xec90, 0x1e928, 0xe920,
    0xd260, 0xa566, 0xa570, 0x4d60, 0x6d54, 0x7550, 0x7490, 0xe933, 0x6930, 0x152b7, 0x52b0,
    0xa5b0, 0x155a5, 0x56a0, 0xb650, 0x174a4, 0xb4a0, 0x1a958, 0xa950, 0x52d0, 0xaad6, 0xab50,
    0x5aa0, 0xba54, 0xda50, 0xd4a0, 0x1c953, 0xc960, 0x194e7, 0x5560, 0xab50, 0x15b25, 0x6d20,
    0xea50, 0xe4a4, 0x68b0, 0xc978, 0x4ab0, 0x55b0, 0xad66, 0xb6a0, 0x7520, 0x17254, 0xb450,
    0xa8b0, 0x149b2, 0x4ab0,
];

fn months(year: i32) -> Option<impl Iterator<Item = (u32, bool, u32)>> {
    let bits = *YEARS.get(usize::try_from(year.checked_sub(1900)?).ok()?)?;
    let leap = bits & 15;
    Some(
        (1..=12)
            .flat_map(move |month| {
                std::iter::once((month, false)).chain((month == leap).then_some((month, true)))
            })
            .enumerate()
            .map(move |(index, (month, leap))| (month, leap, 29 + ((bits >> (4 + index)) & 1))),
    )
}

fn epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(1900, 1, 31).unwrap()
}

pub fn from_solar(date: NaiveDate) -> Option<(i32, u32, bool, u32)> {
    let mut days = (date - epoch()).num_days();
    if days < 0 {
        return None;
    }
    for year in 1900..=2100 {
        for (month, leap, length) in months(year)? {
            if days < i64::from(length) {
                return Some((year, month, leap, days as u32 + 1));
            }
            days -= i64::from(length);
        }
    }
    None
}

pub fn anniversary(year: i32, month: u32, leap: bool, day: u32) -> Option<NaiveDate> {
    if !(1..=30).contains(&day) {
        return None;
    }
    let calendar = months(year)?;
    let use_leap = leap && months(year)?.any(|(m, l, _)| m == month && l);
    let mut offset: i64 =
        (1900..year).flat_map(|y| months(y).unwrap()).map(|(_, _, n)| i64::from(n)).sum();
    for (m, l, n) in calendar {
        if (m, l) == (month, use_leap) {
            return Some(epoch() + Duration::days(offset + i64::from(day.min(n) - 1)));
        }
        offset += i64::from(n);
    }
    None
}

pub fn next_annual(
    today: NaiveDate,
    lunar: bool,
    month: u32,
    leap: bool,
    day: u32,
) -> Option<NaiveDate> {
    let year = if lunar { from_solar(today)?.0 } else { today.year() };
    (year..=year + 8)
        .filter_map(|y| {
            if lunar {
                anniversary(y, month, leap, day)
            } else {
                NaiveDate::from_ymd_opt(y, month, day)
            }
        })
        .find(|d| *d >= today)
}

#[cfg(test)]
#[path = "lunar_tests.rs"]
mod tests;
