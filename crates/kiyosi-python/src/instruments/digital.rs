use crate::conversions::{date, direction, python_date, real, supplied};
use crate::error::domain;
use pyo3::prelude::*;
use pyo3::types::PyDate;

macro_rules! digital_option {
    ($name:ident, $constructor:ident $(, $payout:ident)?, $doc:literal) => {
        #[doc = $doc]
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(Clone, PartialEq)]
        pub(crate) struct $name(pub kiyosi::DigitalOption);

        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = (*, option_type, strike, $($payout,)? effective_midnight_utc, expiry_midnight_utc))]
            fn new(option_type: &Bound<'_, PyAny>, strike: &Bound<'_, PyAny>,
                $($payout: &Bound<'_, PyAny>,)? effective_midnight_utc: &Bound<'_, PyAny>, expiry_midnight_utc: &Bound<'_, PyAny>,
            ) -> PyResult<Self> {
                Ok(Self(domain(kiyosi::DigitalOption::$constructor(direction(option_type)?,
                    real(strike)?, $(real($payout)?,)? date(effective_midnight_utc)?, date(expiry_midnight_utc)?) )?))
            }

            /// Return a validated replacement without changing this value.
            ///
            /// Parameters
            /// ----------
            /// option_type : {'call', 'put'}, optional
            ///     Call or put payoff direction.
            /// strike : float, optional
            ///     Positive finite strike price.
            $(#[doc = concat!(stringify!($payout), " : float, optional\n    Positive finite cash payment.")])?
            /// effective_midnight_utc : datetime.date, optional
            ///     Contract starts at 00:00 UTC on this date.
            /// expiry_midnight_utc : datetime.date, optional
            ///     Expiry boundary at 00:00 UTC; later instants on this date are outside contract life.
            ///
            /// Returns
            /// -------
            /// object
            ///     New instance of the same class; omitted fields or Ellipsis retain their values.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If replacement terms fail domain validation. The original is unchanged.
            #[pyo3(signature = (*, option_type=crate::conversions::OMITTED, strike=crate::conversions::OMITTED, $($payout=crate::conversions::OMITTED,)? effective_midnight_utc=crate::conversions::OMITTED, expiry_midnight_utc=crate::conversions::OMITTED))]
            fn replace(
                &self,
                #[pyo3(from_py_with = supplied)] option_type: Option<&Bound<'_, PyAny>>,
                #[pyo3(from_py_with = supplied)] strike: Option<&Bound<'_, PyAny>>,
                $(#[pyo3(from_py_with = supplied)] $payout: Option<&Bound<'_, PyAny>>,)?
                #[pyo3(from_py_with = supplied)] effective_midnight_utc: Option<&Bound<'_, PyAny>>,
                #[pyo3(from_py_with = supplied)] expiry_midnight_utc: Option<&Bound<'_, PyAny>>,
            ) -> PyResult<Self> {
                let mut changes = kiyosi::DigitalOptionChanges {
                    option_type: option_type.map(direction).transpose()?,
                    strike: strike.map(real).transpose()?,
                    ..Default::default()
                };
                $(changes.payout = $payout.map(real).transpose()?;)?
                changes.effective_midnight_utc = effective_midnight_utc.map(date).transpose()?;
                changes.expiry_midnight_utc = expiry_midnight_utc.map(date).transpose()?;
                Ok(Self(domain(self.0.replace(changes))?))
            }

            /// Return call or put payoff direction.
            #[getter]
            fn option_type(&self) -> &'static str {
                if self.0.option_type() == kiyosi::OptionType::Call { "call" } else { "put" }
            }
            /// Return the strike price in underlying price units.
            #[getter]
            fn strike(&self) -> f64 { self.0.strike() }
            /// Return inclusive first contract date.
            #[getter]
            fn effective_midnight_utc<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
                python_date(py, self.0.effective_midnight_utc())
            }
            /// Return final contract date; expiry is at midnight UTC.
            #[getter]
            fn expiry_midnight_utc<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
                python_date(py, self.0.expiry_midnight_utc())
            }
            $(
                /// Return the fixed cash payout.
                #[getter]
                fn $payout(&self) -> Option<f64> { self.0.payout() }
            )?
        }
    };
}

digital_option!(
    CashOrNothingOption,
    cash_or_nothing,
    payout,
    r"European cash-or-nothing option paying only strictly in the money.

Parameters
----------
option_type : {'call', 'put'}
    Call or put payoff direction.
strike : float
    Positive finite strike price.
payout : float
    Positive finite cash payment.
effective_midnight_utc : datetime.date
    Contract starts at 00:00 UTC on this date.
expiry_midnight_utc : datetime.date
    Expiry boundary at 00:00 UTC; later instants on this date are outside contract life.

Raises
------
InvalidInputError
    If payout or strike is nonpositive or non-finite, or expiry precedes effective_midnight_utc."
);
digital_option!(
    AssetOrNothingOption,
    asset_or_nothing,
    r"European asset-or-nothing option paying only strictly in the money.

Parameters
----------
option_type : {'call', 'put'}
    Call or put payoff direction.
strike : float
    Positive finite strike price.
effective_midnight_utc : datetime.date
    Contract starts at 00:00 UTC on this date.
expiry_midnight_utc : datetime.date
    Expiry boundary at 00:00 UTC; later instants on this date are outside contract life.

Raises
------
InvalidInputError
    If strike is nonpositive or non-finite, or expiry precedes effective_midnight_utc."
);

#[derive(FromPyObject)]
pub(crate) enum DigitalInstrument<'py> {
    Cash(PyRef<'py, CashOrNothingOption>),
    Asset(PyRef<'py, AssetOrNothingOption>),
}
impl DigitalInstrument<'_> {
    pub(crate) fn contract(&self) -> &kiyosi::DigitalOption {
        match self {
            Self::Cash(option) => &option.0,
            Self::Asset(option) => &option.0,
        }
    }
}

macro_rules! with_digital {
    ($instrument:expr, $option:ident, $body:expr) => {{
        let $option = $instrument.contract();
        $body
    }};
}
pub(crate) use with_digital;
