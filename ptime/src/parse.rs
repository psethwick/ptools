use crate::entries::*;
use crate::files::get_day_contents;
use chrono::NaiveDate;
use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{alphanumeric1, digit1, not_line_ending},
    combinator::{map, map_res, not, opt, peek, recognize, value},
    multi::{many0, many1},
    sequence::{preceded, tuple},
    IResult,
};
use std::num::ParseIntError;
use std::str::FromStr;

pub fn parse_time(i: &str) -> IResult<&str, usize> {
    map_res(digit1, hm_to_decihour)(i)
}

fn skip_non_time_line(i: &str) -> IResult<&str, ()> {
    map(tuple((not(digit1), not_line_ending, tag("\n"))), |_| ())(i)
}

fn hm_to_decihour(i: &str) -> Result<usize, ParseIntError> {
    let input = usize::from_str(i)?;
    let hours = (input / 100) * 100;
    let minutes = ((input % 100) * 100) / 60;
    Ok(hours + minutes)
}

fn peek_parse_time(i: &str) -> IResult<&str, Option<usize>> {
    let (input, _) = many0(skip_non_time_line)(i)?;
    alt((map(peek(parse_time), Some), value(None, peek(not(digit1)))))(input)
}

fn parse_client(i: &str) -> IResult<&str, &str> {
    recognize(many1(alt((alphanumeric1, tag("-")))))(i)
}

fn parse_entry_details(i: &str) -> IResult<&str, (EntryType, Option<usize>)> {
    alt((
        map(
            tuple((tag("break"), tag("\n"), peek_parse_time)),
            |(_, _, end)| (EntryType::Break, end),
        ),
        map(
            tuple((
                parse_client,
                opt(preceded(tag(" "), not_line_ending)),
                tag("\n"),
                peek_parse_time,
            )),
            |(client, rest, _, end)| {
                let (task, ticket_id) = if let Some(r) = rest {
                    let mut words: Vec<&str> = r.split_whitespace().collect();
                    if words.is_empty() {
                        ("".to_string(), None)
                    } else {
                        let last_word = words.last().unwrap();
                        let mut ticket_parts = last_word.splitn(2, '-');
                        let is_ticket = if let (Some(key), Some(num)) =
                            (ticket_parts.next(), ticket_parts.next())
                        {
                            !key.is_empty()
                                && key
                                    .chars()
                                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                                && !num.is_empty()
                                && num.chars().all(|c| c.is_ascii_digit())
                        } else {
                            false
                        };

                        if is_ticket {
                            let ticket = words.pop().unwrap().to_string();
                            (words.join(" "), Some(ticket))
                        } else {
                            (r.to_string(), None)
                        }
                    }
                } else {
                    ("".to_string(), None)
                };

                (
                    EntryType::Work {
                        client: client.to_string(),
                        task,
                        ticket_id,
                    },
                    end,
                )
            },
        ),
    ))(i)
}

fn parse_entry(i: &str) -> IResult<&str, Entry> {
    map(
        tuple((parse_time, tag(" "), parse_entry_details)),
        |(start, _, (entry_type, end))| Entry {
            start,
            end,
            entry_type,
        },
    )(i)
}

fn parse_day(i: &str) -> IResult<&str, Vec<Entry>> {
    many0(parse_entry)(i)
}

impl Day {
    pub fn new(date: NaiveDate) -> Option<Self> {
        if let Some(content) = get_day_contents(date) {
            Day::from(date, content)
        } else {
            None
        }
    }

    fn from(date: NaiveDate, content: String) -> Option<Self> {
        match parse_day(content.as_ref()) {
            Ok((_, entries)) => Some(Self { date, entries }),
            Err(e) => {
                println!("{e:?}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::parse::*;
    use chrono::Local;
    const TEST_DAY: &str = "0730 client-a integration tests\n1000 client-b meeting about game\n1100 client-b portal upgrades\n1300 break\n1345 client-b portal upgrades\nsdf\n1400 client-c auth meeting\n1430 client-b portal upgrades\n1530 client-a stand-up\n";
    #[test]
    fn parse() {
        let day = Day::from(Local::now().date_naive(), TEST_DAY.to_string()).unwrap();
        assert_eq!(day.entries.len(), 8);
        assert_eq!(day.entries[0].duration(), Some(2.50));
        assert_eq!(day.total_work(None), 7.25);
        assert_eq!(day.total_work(Some("client-b")), 4.25);
    }

    #[test]
    fn t_parse_time() {
        let t: &str = "1000 client task";
        assert_eq!(parse_time(t), Ok((" client task", 1000)));
    }

    #[test]
    fn t_parse_time_half_hour() {
        let t: &str = "1030 client task";
        assert_eq!(parse_time(t), Ok((" client task", 1050)));
    }

    #[test]
    fn t_parse_time_quarter_hour() {
        let t: &str = "0915 client task";
        assert_eq!(parse_time(t), Ok((" client task", 925)));
    }

    #[test]
    fn t_parse_work() {
        let t: &str = "1000 client task\n";
        assert_eq!(
            parse_entry(t),
            Ok((
                "",
                Entry {
                    start: 1000,
                    end: None,
                    entry_type: EntryType::Work {
                        client: "client".to_string(),
                        task: "task".to_string(),
                        ticket_id: None
                    }
                }
            ))
        );
    }

    #[test]
    fn t_parse_work_with_ticket() {
        let t: &str = "1000 client task CT-1234\n";
        assert_eq!(
            parse_entry(t),
            Ok((
                "",
                Entry {
                    start: 1000,
                    end: None,
                    entry_type: EntryType::Work {
                        client: "client".to_string(),
                        task: "task".to_string(),
                        ticket_id: Some("CT-1234".to_string())
                    }
                }
            ))
        );
    }

    #[test]
    fn t_parse_work_bare_client() {
        let t: &str = "1000 admin\n";
        assert_eq!(
            parse_entry(t),
            Ok((
                "",
                Entry {
                    start: 1000,
                    end: None,
                    entry_type: EntryType::Work {
                        client: "admin".to_string(),
                        task: "".to_string(),
                        ticket_id: None
                    }
                }
            ))
        );
    }

    #[test]
    fn t_parse_break() {
        let t: &str = "1200 break\n";
        assert_eq!(
            parse_entry(t),
            Ok((
                "",
                Entry {
                    start: 1200,
                    end: None,
                    entry_type: EntryType::Break
                }
            ))
        );
    }

    #[test]
    fn t_parse_break_w_end_time() {
        let t: &str = "1200 break
1300 something something";
        assert_eq!(
            parse_entry(t),
            Ok((
                "1300 something something",
                Entry {
                    start: 1200,
                    end: Some(1300),
                    entry_type: EntryType::Break
                }
            ))
        );
    }
}
