use chrono::Datelike;
use chrono::Days;
use chrono::Duration;
use chrono::Months;
use chrono::NaiveDate;

use crate::constants::EXCEL_DATE_BASE;
use crate::constants::MAXIMUM_DATE_SERIAL_NUMBER;
use crate::constants::MINIMUM_DATE_SERIAL_NUMBER;

pub const DATE_OUT_OF_RANGE_MESSAGE: &str = "Out of range parameters for date";

thread_local! {
    static DATE1904: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// CellX: switches this thread's date functions to the 1904 date system.
pub fn set_date1904(on: bool) {
    DATE1904.with(|c| c.set(on));
}

/// CellX: days between the 1900 and the active date system's serial 0.
pub fn date_system_offset() -> i64 {
    if DATE1904.with(|c| c.get()) {
        1462
    } else {
        0
    }
}

/// The serial number of a date in Excel's 1900 calendar. Up to 1900-02-28 it
/// is one less than the days since 1899-12-30, because Excel counts a
/// 1900-02-29 (serial 60) that never was; 1899-12-31 is serial 0 ("1900-01-00").
#[inline]
pub fn convert_to_serial_number(date: NaiveDate) -> i32 {
    let days = date.num_days_from_ce() - EXCEL_DATE_BASE;
    let serial = if days >= 61 { days } else { days - 1 };
    serial - date_system_offset() as i32
}

fn is_date_within_range(date: NaiveDate) -> bool {
    convert_to_serial_number(date) >= MINIMUM_DATE_SERIAL_NUMBER
        && convert_to_serial_number(date) <= MAXIMUM_DATE_SERIAL_NUMBER
}

pub fn from_excel_date(days: i64) -> Result<NaiveDate, String> {
    let offset = date_system_offset();
    if days < MINIMUM_DATE_SERIAL_NUMBER as i64 || days > MAXIMUM_DATE_SERIAL_NUMBER as i64 - offset {
        return Err(DATE_OUT_OF_RANGE_MESSAGE.to_string());
    }
    let days = days + offset;
    if days < MINIMUM_DATE_SERIAL_NUMBER as i64 {
        return Err(format!(
            "Excel date must be greater than {MINIMUM_DATE_SERIAL_NUMBER}"
        ));
    };
    if days > MAXIMUM_DATE_SERIAL_NUMBER as i64 {
        return Err(format!(
            "Excel date must be less than {MAXIMUM_DATE_SERIAL_NUMBER}"
        ));
    };
    #[allow(clippy::expect_used)]
    let base = NaiveDate::from_ymd_opt(1899, 12, 30).expect("problem with chrono::NaiveDate");
    // Excel's calendar: serial 0 is 1899-12-31, 1 is 1900-01-01, and serial 60,
    // the 1900-02-29 that never was, is read as 1900-02-28.
    Ok(match days {
        61.. => base + Duration::days(days),
        60 => base + Duration::days(60),
        _ => base + Duration::days(days + 1),
    })
}

pub fn date_to_serial_number(day: u32, month: u32, year: i32) -> Result<i32, String> {
    match NaiveDate::from_ymd_opt(year, month, day) {
        Some(native_date) => Ok(convert_to_serial_number(native_date)),
        None => Err(DATE_OUT_OF_RANGE_MESSAGE.to_string()),
    }
}

pub fn permissive_date_to_serial_number(day: i32, month: i32, year: i32) -> Result<i32, String> {
    // Excel parses `DATE` very permissively. It allows not just for valid date values, but it
    // allows for invalid dates as well. If you for example enter `DATE(1900, 1, 32)` it will
    // return the date `1900-02-01`. Despite giving a day that is out of range it will just
    // wrap the month and year around.
    //
    // This function applies that same logic to dates. And does it in the most compatible way as
    // possible.

    // Special case for the minimum date
    if year == 1899 && month == 12 && day == 31 {
        return Ok(MINIMUM_DATE_SERIAL_NUMBER);
    }
    let Some(mut date) = NaiveDate::from_ymd_opt(year, 1, 1) else {
        return Err(DATE_OUT_OF_RANGE_MESSAGE.to_string());
    };

    // One thing to note for example is that even if you started with a year out of range
    // but tried to increment the months so that it wraps around into within range, excel
    // would still return an error.
    //
    // I.E. DATE(0,13,-1) will return an error, despite it being equivalent to DATE(1,1,0) which
    // is within range.
    //
    // As a result, we have to run range checks as we parse the date from the biggest unit to the
    // smallest unit.
    if !is_date_within_range(date) {
        return Err(DATE_OUT_OF_RANGE_MESSAGE.to_string());
    }

    date = {
        let month_diff = month - 1;
        let abs_month = month_diff.unsigned_abs();
        // CellX: checked, a huge month count is out of range, not a panic.
        let shifted = if month_diff <= 0 {
            date.checked_sub_months(Months::new(abs_month))
        } else {
            date.checked_add_months(Months::new(abs_month))
        };
        date = match shifted {
            Some(d) => d,
            None => return Err(DATE_OUT_OF_RANGE_MESSAGE.to_string()),
        };
        if !is_date_within_range(date) {
            return Err(DATE_OUT_OF_RANGE_MESSAGE.to_string());
        }
        date
    };

    // CellX: the days are counted on serial numbers, as Excel does, so that
    // DATE(1900,2,29) is 60 and DATE(1900,1,0) is 0.
    let serial = convert_to_serial_number(date) as i64 + day as i64 - 1;
    if serial < MINIMUM_DATE_SERIAL_NUMBER as i64 || serial > MAXIMUM_DATE_SERIAL_NUMBER as i64 {
        return Err(DATE_OUT_OF_RANGE_MESSAGE.to_string());
    }
    let _ = Days::new(0);
    Ok(serial as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permissive_date_to_serial_number() {
        assert_eq!(
            permissive_date_to_serial_number(42, 42, 2002),
            date_to_serial_number(12, 7, 2005)
        );
        assert_eq!(
            permissive_date_to_serial_number(1, 42, 2002),
            date_to_serial_number(1, 6, 2005)
        );
        assert_eq!(
            permissive_date_to_serial_number(1, 15, 2000),
            date_to_serial_number(1, 3, 2001)
        );
        assert_eq!(
            permissive_date_to_serial_number(1, 49, 2000),
            date_to_serial_number(1, 1, 2004)
        );
        assert_eq!(
            permissive_date_to_serial_number(1, 49, 2000),
            date_to_serial_number(1, 1, 2004)
        );
        assert_eq!(
            permissive_date_to_serial_number(31, 49, 2000),
            date_to_serial_number(31, 1, 2004)
        );
        assert_eq!(
            permissive_date_to_serial_number(256, 49, 2000),
            date_to_serial_number(12, 9, 2004)
        );
        assert_eq!(
            permissive_date_to_serial_number(256, 1, 2004),
            date_to_serial_number(12, 9, 2004)
        );
    }

    #[test]
    fn test_max_and_min_dates() {
        assert_eq!(
            permissive_date_to_serial_number(31, 12, 9999),
            Ok(MAXIMUM_DATE_SERIAL_NUMBER),
        );
        assert_eq!(
            permissive_date_to_serial_number(31, 12, 1899),
            Ok(MINIMUM_DATE_SERIAL_NUMBER),
        );
    }
}

/// CellX: a date as Excel's 1900 calendar shows it, with serial 0 as
/// "1900-01-00" and serial 60 as the fictional 1900-02-29.
#[derive(Clone, Copy, Debug)]
pub struct ExcelDate {
    year: i32,
    month: u32,
    day: u32,
    weekday: chrono::Weekday,
}

impl ExcelDate {
    pub fn year(&self) -> i32 {
        self.year
    }
    pub fn month(&self) -> u32 {
        self.month
    }
    pub fn day(&self) -> u32 {
        self.day
    }
    pub fn weekday(&self) -> chrono::Weekday {
        self.weekday
    }
}

pub fn excel_date_parts(serial: i64) -> Result<ExcelDate, String> {
    let date = from_excel_date(serial)?;
    let offset = date_system_offset();
    let (year, month, day) = match serial + offset {
        0 => (1900, 1, 0),
        60 => (1900, 2, 29),
        _ => (date.year(), date.month(), date.day()),
    };
    use chrono::Weekday::*;
    let weekday = [Sun, Mon, Tue, Wed, Thu, Fri, Sat][(serial + offset + 6).rem_euclid(7) as usize];
    Ok(ExcelDate {
        year,
        month,
        day,
        weekday,
    })
}
