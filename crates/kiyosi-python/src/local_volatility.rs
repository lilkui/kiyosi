use crate::conversions::{python_timestamp, real, supplied, timestamp};
use crate::error::domain;
use crate::market::{RateTermStructure, rate};
use pyo3::prelude::*;

/// Immutable local-volatility table, with rows=time and columns=absolute spot.
///
/// Parameters
/// ----------
/// times : iterable of datetime.datetime
///     At least two strictly increasing instants; the first is the reference.
/// spots : iterable of float
///     At least two strictly increasing positive finite prices.
/// values : iterable of iterable of float
///     Rectangular positive finite annualized volatilities (20% is 0.2).
///
/// Raises
/// ------
/// InvalidInputError
///     If the table shape, nodes or values are invalid.
///
/// Notes
/// -----
/// Local volatility interpolates bilinearly in time and absolute spot price,
/// using Actual/365 Fixed year fractions. Time extrapolation is rejected;
/// extrapolation in the spot dimension holds the nearest endpoint volatility constant.
/// Inputs are copied. Properties return owned copies; spot changes and time
/// rolls never translate the surface. Native conversion errors reject booleans,
/// dates, timezone-aware datetimes and invalid iterable elements.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct LocalVolatilitySurface(pub(crate) kiyosi::LocalVolatilitySurface);

#[pymethods]
impl LocalVolatilitySurface {
    #[new]
    #[pyo3(signature = (*, times, spots, values))]
    fn new(
        times: &Bound<'_, PyAny>,
        spots: &Bound<'_, PyAny>,
        values: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        let times = times
            .try_iter()?
            .map(|v| timestamp(&v?))
            .collect::<PyResult<Vec<_>>>()?;
        let spots = spots
            .try_iter()?
            .map(|v| real(&v?))
            .collect::<PyResult<Vec<_>>>()?;
        let values = values
            .try_iter()?
            .map(|row| {
                row?.try_iter()?
                    .map(|v| real(&v?))
                    .collect::<PyResult<Vec<_>>>()
            })
            .collect::<PyResult<Vec<_>>>()?;
        Ok(Self(domain(kiyosi::LocalVolatilitySurface::new(
            times, spots, values,
        ))?))
    }
    /// Return copied absolute time nodes as datetimes.
    #[getter]
    fn times<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyAny>>> {
        self.0
            .times()
            .iter()
            .map(|&t| python_timestamp(py, t))
            .collect()
    }
    /// Return copied absolute price nodes.
    #[getter]
    fn spots(&self) -> Vec<f64> {
        self.0.spots().to_vec()
    }
    /// Return copied volatility rows, indexed by time then price.
    #[getter]
    fn values(&self) -> Vec<Vec<f64>> {
        self.0.values().map(<[f64]>::to_vec).collect()
    }
    /// Return the largest supplied annualized volatility.
    #[getter]
    fn maximum_volatility(&self) -> f64 {
        self.0.maximum_volatility()
    }
    /// Interpolate sigma at an absolute time and positive spot.
    ///
    /// Parameters
    /// ----------
    /// time : datetime.datetime
    ///     Covered query instant.
    /// spot : float
    ///     Positive finite underlying price; extrapolation holds endpoint volatility constant.
    ///
    /// Returns
    /// -------
    /// float
    ///     Annualized local volatility in decimal units.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If time is outside coverage or spot is nonpositive or non-finite.
    fn volatility(&self, time: &Bound<'_, PyAny>, spot: &Bound<'_, PyAny>) -> PyResult<f64> {
        domain(self.0.volatility(timestamp(time)?, real(spot)?))
    }
}

/// Immutable local-volatility parameters with owned rate curves and surface.
///
/// Parameters
/// ----------
/// risk_free_rate, dividend_yield : float or RateTermStructure
///     Finite continuously compounded rates or validated rate curves.
/// surface : LocalVolatilitySurface
///     Validated surface in absolute time and price coordinates.
///
/// Raises
/// ------
/// InvalidInputError
///     If a scalar rate is non-finite.
///
/// Notes
/// -----
/// Surface and curve coverage are checked by the context and during pricing
/// over the required remaining horizons. The one-dimensional finite-difference
/// engines support price, delta and gamma; digital pricing requires explicitly flat rates.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct LocalVolatilityParameters(pub(crate) kiyosi::LocalVolatilityParameters);

#[pymethods]
impl LocalVolatilityParameters {
    #[new]
    #[pyo3(signature = (*, risk_free_rate, dividend_yield, surface))]
    fn new(
        risk_free_rate: &Bound<'_, PyAny>,
        dividend_yield: &Bound<'_, PyAny>,
        surface: &LocalVolatilitySurface,
    ) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::LocalVolatilityParameters::new(
            rate(risk_free_rate)?,
            rate(dividend_yield)?,
            surface.0.clone(),
        ))?))
    }
    /// Return a validated new model; omitted fields or Ellipsis retain their values.
    ///
    /// Parameters
    /// ----------
    /// risk_free_rate, dividend_yield : float or RateTermStructure, optional
    ///     Replacement continuously compounded rates or curves.
    /// surface : LocalVolatilitySurface, optional
    ///     Replacement owned surface.
    ///
    /// Returns
    /// -------
    /// LocalVolatilityParameters
    ///     Independent replacement; the original is unchanged on failure.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If a replacement scalar rate is non-finite.
    #[pyo3(signature = (*, risk_free_rate=crate::conversions::OMITTED, dividend_yield=crate::conversions::OMITTED, surface=crate::conversions::OMITTED))]
    fn replace(
        &self,
        #[pyo3(from_py_with = supplied)] risk_free_rate: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] dividend_yield: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] surface: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        Ok(Self(domain(
            self.0.replace(kiyosi::LocalVolatilityParameterChanges {
                risk_free_rate: risk_free_rate.map(rate).transpose()?,
                dividend_yield: dividend_yield.map(rate).transpose()?,
                surface: surface
                    .map(|v| -> PyResult<_> {
                        Ok(v.extract::<PyRef<'_, LocalVolatilitySurface>>()?.0.clone())
                    })
                    .transpose()?,
            }),
        )?))
    }
    /// Return an owned copy of the fixed surface.
    #[getter]
    fn surface(&self) -> LocalVolatilitySurface {
        LocalVolatilitySurface(self.0.surface().clone())
    }
    /// Return an owned copy of the risk-free curve.
    #[getter]
    fn risk_free_curve(&self) -> RateTermStructure {
        RateTermStructure(self.0.risk_free_curve().clone())
    }
    /// Return an owned copy of the dividend curve.
    #[getter]
    fn dividend_curve(&self) -> RateTermStructure {
        RateTermStructure(self.0.dividend_curve().clone())
    }
    /// Returns the scalar rate for dated or undated explicitly flat curves.
    ///
    /// Raises UnsupportedOperationError for node-based curves; use risk_free_curve
    /// to preserve coverage when passing rates to constructors or replacements.
    #[getter]
    fn flat_risk_free_rate(&self) -> PyResult<f64> {
        domain(self.0.flat_risk_free_rate())
    }
    /// Returns the scalar yield for dated or undated explicitly flat curves.
    ///
    /// Raises UnsupportedOperationError for node-based curves; use dividend_curve
    /// to preserve coverage when passing yields to constructors or replacements.
    #[getter]
    fn flat_dividend_yield(&self) -> PyResult<f64> {
        domain(self.0.flat_dividend_yield())
    }
}
