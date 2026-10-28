use super::*;

pub(crate) fn greek(value: &Bound<'_, PyAny>) -> PyResult<core::Greek> {
    let name = value.extract::<&str>()?;
    core::Greek::ALL
        .into_iter()
        .find(|g| g.name() == name)
        .ok_or_else(|| PyValueError::new_err("unknown Greek name"))
}

pub(crate) fn request(
    greeks: Option<&Bound<'_, PyAny>>,
    settings: Option<&Bound<'_, PyDict>>,
) -> PyResult<core::GreeksRequest> {
    let all = settings
        .map(|s| s.get_item("all_greeks"))
        .transpose()?
        .flatten();
    let all = if let Some(all) = all {
        if !all.is_instance_of::<PyBool>() {
            return Err(PyTypeError::new_err("all_greeks must be bool"));
        }
        all.extract::<bool>()?
    } else {
        false
    };
    let greeks = greeks.filter(|g| !g.is_none());
    if all {
        if greeks.is_some() {
            return Err(PyTypeError::new_err(
                "greeks must be omitted with all_greeks",
            ));
        }
        return Ok(core::GreeksRequest::all());
    }
    let greeks = greeks.ok_or_else(|| PyTypeError::new_err("provide greeks or all_greeks=True"))?;
    let measures = if greeks.is_instance_of::<pyo3::types::PyString>() {
        vec![greek(greeks)?]
    } else {
        greeks
            .try_iter()?
            .map(|item| greek(&item?))
            .collect::<PyResult<Vec<_>>>()?
    };
    Ok(core::GreeksRequest::new(measures))
}

fn real_setting(settings: Option<&Bound<'_, PyDict>>, name: &str, default: f64) -> PyResult<f64> {
    settings
        .map(|s| s.get_item(name))
        .transpose()?
        .flatten()
        .as_ref()
        .map(real)
        .transpose()
        .map(|v| v.unwrap_or(default))
}
fn integer_setting(
    settings: Option<&Bound<'_, PyDict>>,
    name: &str,
    default: i32,
) -> PyResult<i32> {
    settings
        .map(|s| s.get_item(name))
        .transpose()?
        .flatten()
        .as_ref()
        .map(integer)
        .transpose()
        .map(|v| v.unwrap_or(default))
}
pub(crate) fn shifts(
    settings: Option<&Bound<'_, PyDict>>,
) -> PyResult<core::NumericalShiftSettings> {
    shifts_with_defaults(settings, core::NumericalShiftSettings::default())
}

pub(crate) fn shifts_with_defaults(
    settings: Option<&Bound<'_, PyDict>>,
    d: core::NumericalShiftSettings,
) -> PyResult<core::NumericalShiftSettings> {
    check_changes(
        settings,
        &[
            "all_greeks",
            "spot_shift",
            "volatility_shift",
            "rate_shift",
            "time_shift_days",
        ],
    )?;
    Ok(core::NumericalShiftSettings {
        spot_shift: real_setting(settings, "spot_shift", d.spot_shift)?,
        volatility_shift: real_setting(settings, "volatility_shift", d.volatility_shift)?,
        rate_shift: real_setting(settings, "rate_shift", d.rate_shift)?,
        time_shift_days: integer_setting(settings, "time_shift_days", d.time_shift_days)?,
    })
}

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct PricingResult(pub core::PricingResult);

#[pymethods]
impl PricingResult {
    #[getter]
    fn price(&self) -> f64 {
        self.0.price()
    }
    #[getter]
    fn delta(&self) -> Option<f64> {
        self.0.get(core::Greek::Delta)
    }
    #[getter]
    fn gamma(&self) -> Option<f64> {
        self.0.get(core::Greek::Gamma)
    }
    #[getter]
    fn speed(&self) -> Option<f64> {
        self.0.get(core::Greek::Speed)
    }
    #[getter]
    fn theta(&self) -> Option<f64> {
        self.0.get(core::Greek::Theta)
    }
    #[getter]
    fn charm(&self) -> Option<f64> {
        self.0.get(core::Greek::Charm)
    }
    #[getter]
    fn color(&self) -> Option<f64> {
        self.0.get(core::Greek::Color)
    }
    #[getter]
    fn vega(&self) -> Option<f64> {
        self.0.get(core::Greek::Vega)
    }
    #[getter]
    fn vanna(&self) -> Option<f64> {
        self.0.get(core::Greek::Vanna)
    }
    #[getter]
    fn zomma(&self) -> Option<f64> {
        self.0.get(core::Greek::Zomma)
    }
    #[getter]
    fn rho(&self) -> Option<f64> {
        self.0.get(core::Greek::Rho)
    }
    fn require(&self, py: Python<'_>, measure: &Bound<'_, PyAny>) -> PyResult<f64> {
        domain(py, self.0.require(greek(measure)?))
    }
}

#[pyfunction]
#[pyo3(signature = (engine, instrument, context, observed_price, **settings))]
pub(crate) fn implied_volatility(
    py: Python<'_>,
    engine: &Bound<'_, PyAny>,
    instrument: &Bound<'_, PyAny>,
    context: &PricingContext,
    observed_price: &Bound<'_, PyAny>,
    settings: Option<&Bound<'_, PyDict>>,
) -> PyResult<f64> {
    check_changes(
        settings,
        &[
            "lower_bound",
            "upper_bound",
            "price_tolerance",
            "parameter_tolerance",
            "max_iterations",
        ],
    )?;
    let observed = real(observed_price)?;
    let d = core::ImpliedVolatilitySettings::default();
    let controls = core::ImpliedVolatilitySettings {
        lower_bound: real_setting(settings, "lower_bound", d.lower_bound)?,
        upper_bound: real_setting(settings, "upper_bound", d.upper_bound)?,
        price_tolerance: real_setting(settings, "price_tolerance", d.price_tolerance)?,
        parameter_tolerance: real_setting(settings, "parameter_tolerance", d.parameter_tolerance)?,
        max_iterations: integer_setting(settings, "max_iterations", d.max_iterations)?,
    };
    let context = context.0.clone();
    if engine.is_instance_of::<asian::TurnbullWakemanArithmeticAveragePriceEngine>() {
        let option = instrument
            .extract::<PyRef<'_, asian::ArithmeticAveragePriceOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::TurnbullWakemanArithmeticAveragePriceEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    if engine.is_instance_of::<asian::AnalyticGeometricAveragePriceEngine>() {
        let option = instrument
            .extract::<PyRef<'_, asian::GeometricAveragePriceOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::AnalyticGeometricAveragePriceEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloAsianEngine>>() {
        let engine = engine.0;
        return match instrument.extract::<asian::AsianInstrument<'_>>()? {
            asian::AsianInstrument::Geometric(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
            asian::AsianInstrument::Arithmetic(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
        };
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloDigitalEngine>>() {
        let engine = engine.0;
        let option = instrument
            .extract::<digital::DigitalInstrument<'_>>()?
            .contract()
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloVanillaEngine>>() {
        let engine = engine.0;
        return match instrument.extract::<binomial::VanillaInstrument<'_>>()? {
            binomial::VanillaInstrument::European(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
            binomial::VanillaInstrument::American(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
        };
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceVanillaEngine>>()
    {
        let engine = engine.0;
        return match instrument.extract::<binomial::VanillaInstrument<'_>>()? {
            binomial::VanillaInstrument::European(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
            binomial::VanillaInstrument::American(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
        };
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceDigitalEngine>>()
    {
        let engine = engine.0;
        let option = instrument
            .extract::<digital::DigitalInstrument<'_>>()?
            .contract()
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceBarrierEngine>>()
    {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, barrier::BarrierOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceDoubleBarrierEngine>>()
    {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, double_barrier::DoubleBarrierOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceDoubleTouchEngine>>()
    {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, double_touch::DoubleTouchOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if engine.is_instance_of::<quadrature::QuadratureVanillaEngine>() {
        let option = instrument.extract::<PyRef<'_, EuropeanOption>>()?.0.clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::QuadratureVanillaEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    if engine.is_instance_of::<quadrature::QuadratureDigitalEngine>() {
        let option = instrument
            .extract::<digital::DigitalInstrument<'_>>()?
            .contract()
            .clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::QuadratureDigitalEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, binomial::CoxRossRubinsteinVanillaEngine>>() {
        let engine = engine.0;
        let option = instrument.extract::<binomial::VanillaInstrument<'_>>()?;
        return match option {
            binomial::VanillaInstrument::European(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
            binomial::VanillaInstrument::American(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
        };
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, binomial::CoxRossRubinsteinDigitalEngine>>() {
        let engine = engine.0;
        let option = instrument
            .extract::<digital::DigitalInstrument<'_>>()?
            .contract()
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceBinaryBarrierEngine>>()
    {
        let engine = engine.0;
        return match instrument.extract::<binary_barrier::BinaryInstrument<'_>>()? {
            binary_barrier::BinaryInstrument::Binary(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
            binary_barrier::BinaryInstrument::Touch(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
        };
    }
    if engine.is_instance_of::<binary_barrier::AnalyticBinaryBarrierEngine>() {
        return match instrument.extract::<binary_barrier::BinaryInstrument<'_>>()? {
            binary_barrier::BinaryInstrument::Binary(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(
                            &core::AnalyticBinaryBarrierEngine,
                            &option,
                            &context,
                            observed,
                            controls,
                        )
                    }),
                )
            }
            binary_barrier::BinaryInstrument::Touch(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(
                            &core::AnalyticBinaryBarrierEngine,
                            &option,
                            &context,
                            observed,
                            controls,
                        )
                    }),
                )
            }
        };
    }
    if engine.is_instance_of::<double_barrier::AnalyticDoubleBarrierEngine>() {
        let option = instrument
            .extract::<PyRef<'_, double_barrier::DoubleBarrierOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::AnalyticDoubleBarrierEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloBarrierEngine>>() {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, barrier::BarrierOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, barrier::CoxRossRubinsteinBarrierEngine>>() {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, barrier::BarrierOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, barrier::QuadratureBarrierEngine>>() {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, barrier::BarrierOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if engine.is_instance_of::<barrier::AnalyticBarrierEngine>() {
        let option = instrument
            .extract::<PyRef<'_, barrier::BarrierOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::AnalyticBarrierEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    if engine.is_instance_of::<digital::AnalyticDigitalEngine>() {
        let instrument = instrument.extract::<digital::DigitalInstrument<'_>>()?;
        let option = instrument.contract().clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::AnalyticDigitalEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    if engine.is_instance_of::<american::BjerksundStenslandVanillaEngine>() {
        let option = instrument
            .extract::<PyRef<'_, american::AmericanOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::BjerksundStenslandVanillaEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    engine.extract::<PyRef<'_, AnalyticVanillaEngine>>()?;
    let option = instrument.extract::<PyRef<'_, EuropeanOption>>()?.0.clone();
    domain(
        py,
        py.detach(|| {
            core::implied_volatility(
                &core::AnalyticVanillaEngine,
                &option,
                &context,
                observed,
                controls,
            )
        }),
    )
}
