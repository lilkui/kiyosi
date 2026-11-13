use crate::conversions::{direction, python_timestamp, real, supplied, timestamp};
use crate::error::domain;
use pyo3::prelude::*;

macro_rules! digital_option {
    ($name:ident, $constructor:ident $(, $payout:ident)?, $doc:literal) => {
        #[doc = $doc]
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(Clone, PartialEq)]
        pub(crate) struct $name(pub kiyosi::DigitalOption);

        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = (*, option_type, strike, $($payout,)? effective_time, expiry_time))]
            fn new(option_type: &Bound<'_, PyAny>, strike: &Bound<'_, PyAny>,
                $($payout: &Bound<'_, PyAny>,)? effective_time: &Bound<'_, PyAny>, expiry_time: &Bound<'_, PyAny>,
            ) -> PyResult<Self> {
                Ok(Self(domain(kiyosi::DigitalOption::$constructor(direction(option_type)?,
                    real(strike)?, $(real($payout)?,)? timestamp(effective_time)?, timestamp(expiry_time)?) )?))
            }

            /// Return a validated replacement.
            ///
            /// Parameters
            /// ----------
            /// option_type : {'call', 'put'}, optional
            ///     Call or put payoff direction.
            /// strike : float, optional
            ///     Positive finite strike price.
            $(#[doc = concat!(stringify!($payout), " : float, optional\n    Positive finite cash payment.")])?
            /// effective_time : datetime.datetime, optional
            ///     Inclusive contract start.
            /// expiry_time : datetime.datetime, optional
            ///     Inclusive expiry time.
            ///
            /// Returns
            /// -------
            #[doc = stringify!($name)]
            ///     Validated copy; omitted fields or Ellipsis retain their values.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If replacement terms fail domain validation.
            ///
            #[pyo3(signature = (*, option_type=crate::conversions::OMITTED, strike=crate::conversions::OMITTED, $($payout=crate::conversions::OMITTED,)? effective_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED))]
            fn replace(
                &self,
                #[pyo3(from_py_with = supplied)] option_type: Option<&Bound<'_, PyAny>>,
                #[pyo3(from_py_with = supplied)] strike: Option<&Bound<'_, PyAny>>,
                $(#[pyo3(from_py_with = supplied)] $payout: Option<&Bound<'_, PyAny>>,)?
                #[pyo3(from_py_with = supplied)] effective_time: Option<&Bound<'_, PyAny>>,
                #[pyo3(from_py_with = supplied)] expiry_time: Option<&Bound<'_, PyAny>>,
            ) -> PyResult<Self> {
                let mut changes = kiyosi::DigitalOptionChanges {
                    option_type: option_type.map(direction).transpose()?,
                    strike: strike.map(real).transpose()?,
                    ..Default::default()
                };
                $(changes.payout = $payout.map(real).transpose()?;)?
                changes.effective_time = effective_time.map(timestamp).transpose()?;
                changes.expiry_time = expiry_time.map(timestamp).transpose()?;
                Ok(Self(domain(self.0.replace(changes))?))
            }

            /// Return call or put payoff direction.
            #[getter]
            fn option_type(&self) -> &'static str {
                crate::conversions::direction_name(self.0.option_type())
            }
            /// Return the strike price in underlying price units.
            #[getter]
            fn strike(&self) -> f64 { self.0.strike() }
            /// Return inclusive first contract timestamp.
            #[getter]
            fn effective_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
                python_timestamp(py, self.0.effective_time())
            }
            /// Return inclusive expiry time.
            #[getter]
            fn expiry_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
                python_timestamp(py, self.0.expiry_time())
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
effective_time : datetime.datetime
    Inclusive contract start.
expiry_time : datetime.datetime
    Inclusive expiry time.

Raises
------
InvalidInputError
    If payout or strike is nonpositive or non-finite, or expiry precedes
    effective_time.
"
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
effective_time : datetime.datetime
    Inclusive contract start.
expiry_time : datetime.datetime
    Inclusive expiry time.

Raises
------
InvalidInputError
    If strike is nonpositive or non-finite, or expiry precedes
    effective_time.
"
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
