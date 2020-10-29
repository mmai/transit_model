// Copyright (C) 2017 Kisio Digital and/or its affiliates.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License as published by the
// Free Software Foundation, version 3.

// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License for more
// details.

// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>
//! This exposes common information between GTFS and NTFS
//! Mainly 2 common things are grouped here:
//! - Accessibility of some equipments
//! - calendar.txt and calendar_dates.txt format are identical between the GTFS
//!   and NTFS

use crate::model::Collections;
use crate::objects::{self, Date, ExceptionType};
use crate::utils::*;
use crate::utils::{de_from_date_string, ser_from_naive_date};
use crate::vptranslator::translate;
use crate::Result;
use chrono::{self, Datelike, Weekday};
use failure::{bail, format_err, ResultExt};
use log::{info, Level as LogLevel};
use serde::{Deserialize, Serialize};
use skip_error::skip_error_and_log;
use std::{collections::BTreeSet, fs::File, path::Path};
use typed_index_collection::*;

/// Structure to serialize/deserialize the file calendar_dates.txt
#[derive(Serialize, Deserialize, Debug)]
pub struct CalendarDate {
    /// Identifiers of the Service
    pub service_id: String,
    #[serde(
        deserialize_with = "de_from_date_string",
        serialize_with = "ser_from_naive_date"
    )]
    /// Date at which the CalendarDate applies
    pub date: Date,
    /// Is the CalendarDate included or excluded
    pub exception_type: ExceptionType,
}

/// Structure to serialize/deserialize the file calendar.txt
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Calendar {
    /// Identifiers of the Service
    #[serde(rename = "service_id")]
    id: String,
    /// True if the Service is active on Mondays
    #[serde(deserialize_with = "de_from_u8", serialize_with = "ser_from_bool")]
    monday: bool,
    /// True if the Service is active on Tuesdays
    #[serde(deserialize_with = "de_from_u8", serialize_with = "ser_from_bool")]
    tuesday: bool,
    /// True if the Service is active on Wednesdays
    #[serde(deserialize_with = "de_from_u8", serialize_with = "ser_from_bool")]
    wednesday: bool,
    /// True if the Service is active on Thursdays
    #[serde(deserialize_with = "de_from_u8", serialize_with = "ser_from_bool")]
    thursday: bool,
    /// True if the Service is active on Fridays
    #[serde(deserialize_with = "de_from_u8", serialize_with = "ser_from_bool")]
    friday: bool,
    /// True if the Service is active on Saturdays
    #[serde(deserialize_with = "de_from_u8", serialize_with = "ser_from_bool")]
    saturday: bool,
    /// True if the Service is active on Sundays
    #[serde(deserialize_with = "de_from_u8", serialize_with = "ser_from_bool")]
    sunday: bool,
    /// The Service is active starting from this date
    #[serde(
        deserialize_with = "de_from_date_string",
        serialize_with = "ser_from_naive_date"
    )]
    start_date: Date,
    /// The Service is active until this date
    #[serde(
        deserialize_with = "de_from_date_string",
        serialize_with = "ser_from_naive_date"
    )]
    end_date: Date,
}

impl Calendar {
    fn get_valid_days(&self) -> Vec<Weekday> {
        let mut valid_days: Vec<Weekday> = vec![];
        if self.monday {
            valid_days.push(Weekday::Mon);
        }
        if self.tuesday {
            valid_days.push(Weekday::Tue);
        }
        if self.wednesday {
            valid_days.push(Weekday::Wed);
        }
        if self.thursday {
            valid_days.push(Weekday::Thu);
        }
        if self.friday {
            valid_days.push(Weekday::Fri);
        }
        if self.saturday {
            valid_days.push(Weekday::Sat);
        }
        if self.sunday {
            valid_days.push(Weekday::Sun);
        }

        valid_days
    }

    fn get_valid_dates(&self) -> BTreeSet<Date> {
        let valid_days = self.get_valid_days();
        let duration = self.end_date - self.start_date;
        (0..=duration.num_days())
            .map(|i| self.start_date + chrono::Duration::days(i))
            .filter(|d| valid_days.contains(&d.weekday()))
            .collect()
    }
}

fn parse_calendar(calendar_file: File) -> Result<CollectionWithId<objects::Calendar>> {
    let mut rdr = csv::Reader::from_reader(calendar_file);
    let mut calendars = CollectionWithId::default();
    for calendar in rdr.deserialize() {
        let calendar: Calendar = calendar?;
        let dates = calendar.get_valid_dates();
        if !dates.is_empty() {
            calendars.push(objects::Calendar {
                id: calendar.id.clone(),
                dates,
            })?;
        }
    }
    Ok(calendars)
}

fn parse_calendar_dates(
    calendar_dates_file: File,
    calendars: &mut CollectionWithId<objects::Calendar>,
) -> Result<()> {
    let mut rdr = csv::Reader::from_reader(calendar_dates_file);
    for calendar_date in rdr.deserialize() {
        let calendar_date: CalendarDate = calendar_date?;

        let is_inserted = calendars
            .get_mut(&calendar_date.service_id)
            .map(|mut calendar| match calendar_date.exception_type {
                ExceptionType::Add => {
                    calendar.dates.insert(calendar_date.date);
                }
                ExceptionType::Remove => {
                    calendar.dates.remove(&calendar_date.date);
                }
            });
        is_inserted.unwrap_or_else(|| {
            if calendar_date.exception_type == ExceptionType::Add {
                let mut dates = BTreeSet::new();
                dates.insert(calendar_date.date);
                calendars
                    .push(objects::Calendar {
                        id: calendar_date.service_id,
                        dates,
                    })
                    .unwrap();
            }
        });
    }
    Ok(())
}

pub(crate) fn manage_calendars<P>(ntfs_path: P, collections: &mut Collections) -> Result<()>
where
    P: AsRef<Path>,
{
    let calendar_path = ntfs_path.as_ref().join("calendar.txt");
    let calendar_dates_path = ntfs_path.as_ref().join("calendar_dates.txt");
    if !calendar_path.exists() && !calendar_dates_path.exists() {
        bail!("calendar_dates.txt or calendar.txt not found");
    }
    let mut calendars = if calendar_path.exists() {
        let calendar_file = File::open(&calendar_path)
            .with_context(|_| format!("Error reading {:?}", calendar_path))?;
        info!("Reading {:?}", calendar_path);
        parse_calendar(calendar_file)?
    } else {
        info!("Skipping {:?}", calendar_path);
        CollectionWithId::new(vec![])?
    };
    if calendar_dates_path.exists() {
        let calendar_dates_file = File::open(&calendar_dates_path)
            .with_context(|_| format!("Error reading {:?}", calendar_dates_path))?;
        info!("Reading {:?}", calendar_dates_path);
        parse_calendar_dates(calendar_dates_file, &mut calendars)?;
    } else {
        info!("Skipping {:?}", calendar_dates_path);
    }
    collections.calendars = calendars;

    Ok(())
}

/// Write the calendar_dates.txt file into a Path from a list of Calendar
pub fn write_calendar_dates(
    path: &Path,
    calendars: &CollectionWithId<objects::Calendar>,
) -> Result<()> {
    info!("Writing calendar_dates.txt");
    let calendar_dates_path = path.join("calendar_dates.txt");
    let mut translations: Vec<Calendar> = vec![];
    let mut exceptions: Vec<CalendarDate> = vec![];
    for c in calendars.values() {
        let translation = translate(&c.dates);
        if !translation.operating_days.is_empty() {
            let validity_period = skip_error_and_log!(
                translation.validity_period.ok_or_else(|| format_err!(
                    "Validity period not found for service id {}",
                    c.id.clone()
                )),
                LogLevel::Warn
            );
            translations.push(Calendar {
                id: c.id.clone(),
                monday: translation.operating_days.contains(&Weekday::Mon),
                tuesday: translation.operating_days.contains(&Weekday::Tue),
                wednesday: translation.operating_days.contains(&Weekday::Wed),
                thursday: translation.operating_days.contains(&Weekday::Thu),
                friday: translation.operating_days.contains(&Weekday::Fri),
                saturday: translation.operating_days.contains(&Weekday::Sat),
                sunday: translation.operating_days.contains(&Weekday::Sun),
                start_date: validity_period.start_date,
                end_date: validity_period.end_date,
            });
        };
        for e in translation.exceptions {
            exceptions.push(CalendarDate {
                service_id: c.id.clone(),
                date: e.date,
                exception_type: e.exception_type,
            });
        }
    }
    if !exceptions.is_empty() {
        let mut wtr = csv::Writer::from_path(&calendar_dates_path)
            .with_context(|_| format!("Error reading {:?}", calendar_dates_path))?;
        for e in exceptions {
            wtr.serialize(&e)
                .with_context(|_| format!("Error reading {:?}", calendar_dates_path))?;
        }
        wtr.flush()
            .with_context(|_| format!("Error reading {:?}", calendar_dates_path))?;
    }
    write_calendar(path, &translations)
}

/// Write the calendar.txt file into a Path from a list of Calendar
pub fn write_calendar(path: &Path, calendars: &[Calendar]) -> Result<()> {
    info!("Writing calendar.txt");
    if calendars.is_empty() {
        return Ok(());
    }

    let calendar_path = path.join("calendar.txt");
    let mut wtr = csv::Writer::from_path(&calendar_path)
        .with_context(|_| format!("Error reading {:?}", calendar_path))?;
    for calendar in calendars {
        wtr.serialize(calendar)
            .with_context(|_| format!("Error reading {:?}", calendar_path))?;
    }
    wtr.flush()
        .with_context(|_| format!("Error reading {:?}", calendar_path))?;
    Ok(())
}
