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
            #[pyo3(signature = (*, option_type, strike, $($payout,)? effective_date, expiry_date))]
            fn new(option_type: &Bound<'_, PyAny>, strike: &Bound<'_, PyAny>,
                $($payout: &Bound<'_, PyAny>,)? effective_date: &Bound<'_, PyAny>, expiry_date: &Bound<'_, PyAny>,
            ) -> PyResult<Self> {
                Ok(Self(domain(kiyosi::DigitalOption::$constructor(direction(option_type)?,
                    real(strike)?, $(real($payout)?,)? date(effective_date)?, date(expiry_date)?) )?))
            }

            /// Return a validated replacement without changing this value.
            ///
            /// Parameters
            /// ----------
            /// option_type : {'call', 'put'}, optional
            ///     Call or put payoff direction.
            /// strike : float, optional
            ///     Positive finite exercise or purchase price.
            /// payout : float, optional
            ///     Positive finite cash payment; accepted only for CashOrNothingOption.
            /// effective_date : datetime.date, optional
            ///     Inclusive first contract date.
            /// expiry_date : datetime.date, optional
            ///     Final contract date; expiry is at midnight UTC.
            ///
            /// Returns
            /// -------
            /// object
            ///     New instance of the same class; omitted fields retain their values.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If replacement terms fail domain validation. The original is unchanged.
            #[pyo3(signature = (*, option_type=None, strike=None, $($payout=None,)? effective_date=None, expiry_date=None))]
            fn replace(
                &self,
                #[pyo3(from_py_with = supplied)] option_type: Option<&Bound<'_, PyAny>>,
                #[pyo3(from_py_with = supplied)] strike: Option<&Bound<'_, PyAny>>,
                $(#[pyo3(from_py_with = supplied)] $payout: Option<&Bound<'_, PyAny>>,)?
                #[pyo3(from_py_with = supplied)] effective_date: Option<&Bound<'_, PyAny>>,
                #[pyo3(from_py_with = supplied)] expiry_date: Option<&Bound<'_, PyAny>>,
            ) -> PyResult<Self> {
                let mut changes = kiyosi::DigitalOptionChanges {
                    option_type: option_type.map(direction).transpose()?,
                    strike: strike.map(real).transpose()?,
                    ..Default::default()
                };
                $(changes.payout = $payout.map(real).transpose()?;)?
                changes.effective_date = effective_date.map(date).transpose()?;
                changes.expiry_date = expiry_date.map(date).transpose()?;
                Ok(Self(domain(self.0.replace(changes))?))
            }

            /// Return call or put payoff direction.
            #[getter]
            fn option_type(&self) -> &'static str {
                if self.0.option_type() == kiyosi::OptionType::Call { "call" } else { "put" }
            }
            /// Return the exercise or purchase price in underlying price units.
            #[getter]
            fn strike(&self) -> f64 { self.0.strike() }
            /// Return inclusive first contract date.
            #[getter]
            fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
                python_date(py, self.0.effective_date())
            }
            /// Return final contract date; expiry is at midnight UTC.
            #[getter]
            fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
                python_date(py, self.0.expiry_date())
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
    r"European digital paying cash strictly in the money.

Parameters
----------
option_type : {'call', 'put'}
    Call or put payoff direction.
strike : float
    Positive finite exercise or purchase price.
payout : float
    Positive finite cash payment.
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.

Raises
------
InvalidInputError
    If payout or strike is nonpositive/non-finite, or expiry precedes effective_date."
);
digital_option!(
    AssetOrNothingOption,
    asset_or_nothing,
    r"European digital paying the asset strictly in the money.

Parameters
----------
option_type : {'call', 'put'}
    Call or put payoff direction.
strike : float
    Positive finite exercise or purchase price.
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.

Raises
------
InvalidInputError
    If strike is nonpositive/non-finite, or expiry precedes effective_date."
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
