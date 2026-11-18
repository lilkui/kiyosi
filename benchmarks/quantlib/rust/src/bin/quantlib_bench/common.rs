use std::{collections::BTreeMap, error::Error, str::FromStr};

use kiyosi::{Date, OptionType, Timestamp};

pub type Price = Box<dyn Fn() -> Result<f64, kiyosi::Error>>;
pub type Failure = Box<dyn Error>;

pub struct Request(BTreeMap<String, String>);

impl Request {
    pub fn parse() -> Result<Self, Failure> {
        let mut values = BTreeMap::new();
        for argument in std::env::args().skip(1) {
            let (key, value) = argument.split_once('=').ok_or("expected key=value")?;
            if values.insert(key.to_owned(), value.to_owned()).is_some() {
                return Err(format!("duplicate parameter: {key}").into());
            }
        }
        Ok(Self(values))
    }

    pub fn take(&mut self, key: &str) -> Result<String, Failure> {
        self.0
            .remove(key)
            .ok_or_else(|| format!("missing parameter: {key}").into())
    }

    pub fn number<T: FromStr>(&mut self, key: &str) -> Result<T, Failure>
    where
        T::Err: Error + 'static,
    {
        Ok(self.take(key)?.parse()?)
    }

    pub fn date(&mut self, key: &str) -> Result<Timestamp, Failure> {
        let value = self.take(key)?;
        let parts: Vec<&str> = value.split('-').collect();
        if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
            return Err(format!("invalid ISO date: {value}").into());
        }
        Ok(Date::new(parts[0].parse()?, parts[1].parse()?, parts[2].parse()?)?.start_of_day())
    }

    pub fn option(&mut self) -> Result<OptionType, Failure> {
        match self.take("option")?.as_str() {
            "call" => Ok(OptionType::Call),
            "put" => Ok(OptionType::Put),
            value => Err(format!("invalid option: {value}").into()),
        }
    }

    pub fn finish(self) -> Result<(), Failure> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(format!("unknown parameters: {:?}", self.0.keys()).into())
        }
    }
}
