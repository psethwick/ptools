/** A tiny tagged language for defining what should be done with the text
* >sink stick the text somewhere (e.g. a note somewhere or a todo item)
* > @agent send to an external agent (e.g. ai, teammate)
* > $cmd send it to a command line tool
*/
use nom::{
    branch::alt, bytes::complete::tag, character::complete::alpha1, sequence::preceded, IResult,
};

fn parse_verb(i: &str) -> IResult<&str, Verb> {
    alt((parse_agent, parse_sink, parse_cmd))(i)
}

fn parse_agent(i: &str) -> IResult<&str, Verb> {
    let (input, v) = preceded(tag("@"), alpha1)(i)?;
    Ok((input, Verb::Agent(v.to_owned())))
}

fn parse_sink(i: &str) -> IResult<&str, Verb> {
    let (input, v) = preceded(tag(">"), alpha1)(i)?;
    Ok((input, Verb::Sink(v.to_owned())))
}

fn parse_cmd(i: &str) -> IResult<&str, Verb> {
    let (input, v) = preceded(tag("$"), alpha1)(i)?;
    Ok((input, Verb::Cmd(v.to_owned())))
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verb {
    Agent(String),
    Cmd(String),
    Sink(String),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Parsed {
    pub text: String,
    pub verb: Option<Verb>,
}

pub fn parse(input: &str) -> Parsed {
    let mut verb: Option<Verb> = None;
    let text: Option<String> = input
        .split(' ')
        .rev()
        .enumerate()
        .filter_map(|(i, word)| {
            if i == 0 {
                if let Ok(v) = parse_verb(word) {
                    verb = v.1.into();
                    return None;
                }
            }
            Some(word.to_owned())
        })
        .reduce(|acc, w| format!("{w} {acc}")) // sneaky re rev
        .to_owned();

    match text {
        Some(text) => Parsed { text, verb },
        None => Parsed {
            text: "".to_owned(),
            verb,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_verb() {
        const INPUT: &str = ">todo";
        assert_eq!(parse_verb(INPUT), Ok(("", Verb::Sink("todo".to_owned()))));
    }
    #[test]
    fn test_parse_input_single_verb() {
        const INPUT: &str = "buy milk >todo";
        assert_eq!(
            parse(INPUT),
            Parsed {
                text: "buy milk".into(),
                verb: Verb::Sink("todo".to_owned()).into()
            }
        );
    }
}
