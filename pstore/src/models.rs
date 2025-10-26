use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Database, Decode, Encode, FromRow, Type};
use std::fmt::Display;

#[derive(PartialEq, Eq, Clone, Debug, Copy, Serialize, Deserialize)]
pub enum Kind {
    AzureDevops = 0,
    Jira = 1,
}

impl Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let printable = match self {
            Kind::AzureDevops => "azure_devops",
            Kind::Jira => "jira",
        };
        write!(f, "{printable}")
    }
}

impl TryFrom<i32> for Kind {
    type Error = String;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Kind::AzureDevops),
            1 => Ok(Kind::Jira),
            _ => Err(format!("Invalid kind value: {value}")),
        }
    }
}

impl From<Kind> for i32 {
    fn from(kind: Kind) -> Self {
        kind as i32
    }
}

impl<DB: Database> Type<DB> for Kind
where
    i32: Type<DB>,
{
    fn type_info() -> DB::TypeInfo {
        <i32 as Type<DB>>::type_info()
    }
}

impl<'r, DB: Database> Decode<'r, DB> for Kind
where
    i32: Decode<'r, DB>,
{
    fn decode(value: <DB as Database>::ValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let n = <i32 as Decode<DB>>::decode(value)?;
        Kind::try_from(n).map_err(Into::into)
    }
}

impl<'q, DB: Database> Encode<'q, DB> for Kind
where
    i32: Encode<'q, DB>,
{
    fn encode_by_ref(
        &self,
        buf: &mut <DB as Database>::ArgumentBuffer<'q>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        let value: i32 = (*self).into();
        <i32 as Encode<DB>>::encode_by_ref(&value, buf)
    }
}

#[derive(PartialEq, Eq, Clone, FromRow, Debug, Serialize, Deserialize)]
pub struct Remote {
    pub id: i64,
    #[sqlx(try_from = "i32")]
    pub kind: Kind,
    pub name: String,
}

#[derive(FromRow, Debug, Serialize, Deserialize)]
pub struct Work {
    pub remote_id: i64,
    pub project: String,
    pub id: String,
    pub title: String,
    pub parent_id: Option<String>,
    pub description: Option<String>,
    pub work_type: String,
    pub version: Option<String>,
    pub state: Option<String>,
    pub created_by_id: Option<String>,
    pub assigned_to_id: Option<String>,
    pub column: Option<String>,
    pub created: Option<DateTime<Utc>>,
    pub modified: Option<DateTime<Utc>>,
    pub url: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Person {
    pub remote_id: i64,
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Data {
    pub work: Vec<Work>,
    pub people: Vec<Person>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Timesheet {
    pub remote_id: i64,
    pub ticket_id: String,
    pub date: String,
    pub duration_seconds: i64,
}
