use anyhow::Result;

use crate::{
    azure_devops::AzureDevops,
    source::{Source, get_password},
};

pub enum SourceConfig {
    AzureDevops(String), // tenant name
}

pub enum Sources {
    AzureDevops(AzureDevops),
}

// add needs to have secret
// then config + get secret to construct

pub fn from_config(sc: SourceConfig) -> Result<impl Source> {
    match sc {
        SourceConfig::AzureDevops(ado) => {
            get_password(&AzureDevops::kind(), &ado).map(|pat| AzureDevops { org: ado, pat })
        }
    }
}
