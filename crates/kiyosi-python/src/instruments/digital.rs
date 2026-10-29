use crate::conversions::{check_changes, date, direction, python_date, real};
use crate::error::domain;
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDict};

fn changes(fields: Option<&Bound<'_, PyDict>>, cash: bool) -> PyResult<core::DigitalOptionChanges> {
    let allowed = if cash {
        &[
            "option_type",
            "strike",
            "payout",
            "effective_date",
            "expiry_date",
        ][..]
    } else {
        &["option_type", "strike", "effective_date", "expiry_date"][..]
    };
    check_changes(fields, allowed)?;
    let item = |name| {
        fields
            .map(|f| f.get_item(name))
            .transpose()
            .map(Option::flatten)
    };
    Ok(core::DigitalOptionChanges {
        option_type: item("option_type")?.as_ref().map(direction).transpose()?,
        strike: item("strike")?.as_ref().map(real).transpose()?,
        payout: item("payout")?.as_ref().map(real).transpose()?,
        effective_date: item("effective_date")?.as_ref().map(date).transpose()?,
        expiry_date: item("expiry_date")?.as_ref().map(date).transpose()?,
    })
}

macro_rules! digital_option {
    ($name:ident, $constructor:ident $(, $payout:ident)?, $doc:literal) => {
        #[doc = $doc]
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(Clone, PartialEq)]
        pub(crate) struct $name(pub core::DigitalOption);

        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = (*, option_type, strike, $($payout,)? effective_date, expiry_date))]
            fn new(py: Python<'_>, option_type: &Bound<'_, PyAny>, strike: &Bound<'_, PyAny>,
                $($payout: &Bound<'_, PyAny>,)? effective_date: &Bound<'_, PyAny>, expiry_date: &Bound<'_, PyAny>,
            ) -> PyResult<Self> {
                Ok(Self(domain(py, core::DigitalOption::$constructor(direction(option_type)?,
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
            #[pyo3(signature = (**fields))]
            fn replace(&self, py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
                let changes = changes(fields, self.0.payout().is_some())?;
                Ok(Self(domain(py, self.0.replace(changes))?))
            }

            /// Return call or put payoff direction.
            #[getter]
            fn option_type(&self) -> &'static str {
                if self.0.option_type() == core::OptionType::Call { "call" } else { "put" }
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
    pub(crate) fn contract(&self) -> &core::DigitalOption {
        match self {
            Self::Cash(option) => &option.0,
            Self::Asset(option) => &option.0,
        }
    }
}
