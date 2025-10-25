use chrono::{Datelike, Duration, Local, Months, NaiveDate, Weekday};
use clap::{self, Parser, Subcommand};
use timesheet::entries::Day;
use timesheet::files::{add_today_entry, get_today_path};
#[derive(Debug, Parser)]
#[command(name = "ptime")]
#[command(about = "Managing your timesheets", long_about = None)]
struct Cli {
    #[arg(short = 'c', long)]
    client: Option<String>,
    #[arg(short = 's', long)]
    sync: Option<bool>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(arg_required_else_help = true)]
    Add {
        time: String,
        #[clap(trailing_var_arg = true)]
        details: Vec<String>,
    },
    Day {
        date: Option<String>,
    },
    Week,
    Month,
    Range {
        start: String,
        end: String,
    },
    Path,
}

fn parse_date(i: &str) -> NaiveDate {
    match NaiveDate::parse_from_str(i, "%Y-%m-%d") {
        Ok(d) => d,
        Err(_) => panic!("invalid date: {i}"),
    }
}

fn report_range(start: NaiveDate, end: NaiveDate, client_filter: Option<&str>) {
    assert!(start < end, "start date must be before end date");
    let mut total_work: f64 = 0.0;

    let mut s: NaiveDate = start;
    while s <= end {
        let day = Day::new(s);
        if let Some(d) = day {
            print!("{}", d.report_str(client_filter));
            total_work += d.total_work(client_filter);
        }
        s += Duration::days(1);
    }
    println!("total work: {total_work}");
}

fn main() {
    let args = Cli::parse();

    match args.command {
        Commands::Range { start, end } => {
            let s = parse_date(&start);
            let e = parse_date(&end);
            report_range(s, e, args.client.as_deref())
        }
        Commands::Week => {
            let today = Local::now().date_naive();
            let mut start = today;
            while start.weekday() != Weekday::Mon {
                start -= Duration::days(1);
            }
            let end = start + Duration::days(4);
            // start = Monday, end = Friday
            report_range(start, end, args.client.as_deref())
        }
        Commands::Month => {
            let today = Local::now().date_naive();
            let mut start = today;
            while start.day() != 1 {
                start -= Duration::days(1);
            }
            let mut end = start;
            end = end.checked_add_months(Months::new(1)).unwrap();
            end -= Duration::days(1);

            report_range(start, end, args.client.as_deref());
        }
        Commands::Day { date } => {
            let day = Day::new(match date {
                Some(date_str) => parse_date(&date_str),
                None => Local::now().date_naive(),
            });

            match day {
                Some(d) => {
                    println!("{}", d.report_str(args.client.as_deref()));
                }
                None => println!("nothing to see here, boss"),
            }
        }
        Commands::Add { time, details } => {
            let joined_deets: String =
                itertools::Itertools::intersperse(details.iter().cloned(), " ".to_string())
                    .collect();
            let entry = format!("{time} {joined_deets}");
            match add_today_entry(&entry) {
                Ok(()) => {}
                Err(e) => println!("{e:?}"),
            }
        }
        Commands::Path => {
            let path = get_today_path();
            let d = path.display();
            println!("{d}");
        }
    }
}
