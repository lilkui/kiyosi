use super::*;

#[pyfunction]
#[pyo3(signature = (engine, instrument, context, observed_price, **settings))]
pub(crate) fn implied_coupon(
    py: Python<'_>,
    engine: &QuadratureAutocallableEngine,
    instrument: &Bound<'_, PyAny>,
    context: &PricingContext,
    observed_price: &Bound<'_, PyAny>,
    settings: Option<&Bound<'_, PyDict>>,
) -> PyResult<f64> {
    check_changes(
        settings,
        &[
            "quote_convention",
            "lower_bound",
            "upper_bound",
            "price_tolerance",
            "parameter_tolerance",
            "max_iterations",
        ],
    )?;
    let observed = real(observed_price)?;
    let d = core::ImpliedCouponSettings::default();
    let controls = core::ImpliedCouponSettings {
        lower_bound: analytics::real_setting(settings, "lower_bound", d.lower_bound)?,
        upper_bound: analytics::real_setting(settings, "upper_bound", d.upper_bound)?,
        price_tolerance: analytics::real_setting(settings, "price_tolerance", d.price_tolerance)?,
        parameter_tolerance: analytics::real_setting(
            settings,
            "parameter_tolerance",
            d.parameter_tolerance,
        )?,
        max_iterations: analytics::integer_setting(settings, "max_iterations", d.max_iterations)?,
    };
    let convention = settings
        .map(|s| s.get_item("quote_convention"))
        .transpose()?
        .flatten();
    let engine = engine.0;
    let context = context.0.clone();
    if let Ok(note) = instrument.extract::<PyRef<'_, PhoenixOption>>() {
        if convention.is_some() {
            return Err(PyTypeError::new_err(
                "Phoenix has no quote_convention argument",
            ));
        }
        let note = note.0.clone();
        return domain(
            py,
            py.detach(|| core::implied_coupon(&engine, &note, &context, observed, controls)),
        );
    }
    let convention =
        convention.ok_or_else(|| PyTypeError::new_err("snowballs require quote_convention"))?;
    let convention = match convention.extract::<&str>()? {
        "shift_maturity_coupon" => core::CouponQuoteConvention::ShiftMaturityCoupon,
        "preserve_maturity_coupon" => core::CouponQuoteConvention::PreserveMaturityCoupon,
        _ => return Err(PyValueError::new_err("unknown coupon quote convention")),
    };
    macro_rules! solve {
        ($note:expr) => {{
            let note = $note.0.clone();
            domain(
                py,
                py.detach(|| {
                    core::implied_snowball_coupon(
                        &engine, &note, &context, observed, convention, controls,
                    )
                }),
            )
        }};
    }
    if let Ok(note) = instrument.extract::<PyRef<'_, BinarySnowballOption>>() {
        return solve!(note);
    }
    if let Ok(note) = instrument.extract::<PyRef<'_, TernarySnowballOption>>() {
        return solve!(note);
    }
    let note = instrument.extract::<PyRef<'_, SnowballOption>>()?;
    solve!(note)
}

fn state(value: &Bound<'_, PyAny>) -> PyResult<Option<core::AutocallableBarrierState>> {
    if value.is_none() {
        return Ok(None);
    }
    Ok(Some(match value.extract::<&str>()? {
        "none" => core::AutocallableBarrierState::None,
        "knocked_in" => core::AutocallableBarrierState::KnockedIn,
        "knocked_out" => core::AutocallableBarrierState::KnockedOut,
        _ => return Err(PyValueError::new_err("unknown autocallable barrier_state")),
    }))
}
fn state_name(value: Option<core::AutocallableBarrierState>) -> Option<&'static str> {
    value.map(|s| match s {
        core::AutocallableBarrierState::None => "none",
        core::AutocallableBarrierState::KnockedIn => "knocked_in",
        core::AutocallableBarrierState::KnockedOut => "knocked_out",
    })
}
fn mode(value: &Bound<'_, PyAny>) -> PyResult<core::KnockInObservationMode> {
    match value.extract::<&str>()? {
        "every_trading_day" => Ok(core::KnockInObservationMode::EveryTradingDay),
        "at_expiry" => Ok(core::KnockInObservationMode::AtExpiry),
        _ => Err(PyValueError::new_err("unknown knock_in_observation_mode")),
    }
}
fn mode_name(value: core::KnockInObservationMode) -> &'static str {
    match value {
        core::KnockInObservationMode::EveryTradingDay => "every_trading_day",
        core::KnockInObservationMode::AtExpiry => "at_expiry",
    }
}
fn records(values: &Bound<'_, PyAny>) -> PyResult<Vec<core::AutocallableObservation>> {
    values
        .try_iter()?
        .map(|item| Ok(item?.extract::<PyRef<'_, AutocallableObservation>>()?.0))
        .collect()
}
fn reals(values: &Bound<'_, PyAny>) -> PyResult<Vec<f64>> {
    values.try_iter()?.map(|item| real(&item?)).collect()
}

/// Owns an input observation; note construction performs domain validation.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct AutocallableObservation(pub core::AutocallableObservation);
#[pymethods]
impl AutocallableObservation {
    #[new]
    #[pyo3(signature = (date, knock_out_level, **fields))]
    fn new(
        date: &Bound<'_, PyAny>,
        knock_out_level: &Bound<'_, PyAny>,
        fields: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        check_changes(fields, &["coupon_rate", "coupon_barrier_level"])?;
        let mut record =
            core::AutocallableObservation::new(super::date(date)?, real(knock_out_level)?);
        record.coupon_rate = analytics::real_setting(fields, "coupon_rate", record.coupon_rate)?;
        record.coupon_barrier_level =
            analytics::real_setting(fields, "coupon_barrier_level", record.coupon_barrier_level)?;
        Ok(Self(record))
    }
    #[getter]
    fn date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.date)
    }
    #[getter]
    fn knock_out_level(&self) -> f64 {
        self.0.knock_out_level
    }
    #[getter]
    fn coupon_rate(&self) -> f64 {
        self.0.coupon_rate
    }
    #[getter]
    fn coupon_barrier_level(&self) -> f64 {
        self.0.coupon_barrier_level
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "AutocallableObservation({}, {:?}, coupon_rate={:?}, coupon_barrier_level={:?})",
            self.date(py)?.repr()?,
            self.0.knock_out_level,
            self.0.coupon_rate,
            self.0.coupon_barrier_level
        ))
    }
}

#[pyfunction]
#[pyo3(signature = (observation_dates, knock_out_levels, *, knock_out_coupon_rates=None, coupon_barrier_levels=None))]
pub(crate) fn autocallable_observations(
    py: Python<'_>,
    observation_dates: &Bound<'_, PyAny>,
    knock_out_levels: &Bound<'_, PyAny>,
    knock_out_coupon_rates: Option<&Bound<'_, PyAny>>,
    coupon_barrier_levels: Option<&Bound<'_, PyAny>>,
) -> PyResult<Vec<AutocallableObservation>> {
    let dates = dates(observation_dates)?;
    let levels = reals(knock_out_levels)?;
    let coupons = knock_out_coupon_rates.map(reals).transpose()?;
    let barriers = coupon_barrier_levels.map(reals).transpose()?;
    Ok(domain(
        py,
        core::autocallable_observations(&dates, &levels, coupons.as_deref(), barriers.as_deref()),
    )?
    .into_iter()
    .map(AutocallableObservation)
    .collect())
}

macro_rules! snowball_preset {
    ($name:ident, $coupon:ident, $level:ident, [$($extra:ident),*], $preset:expr) => {
        #[pyfunction]
        #[pyo3(signature = (*, $coupon, initial_spot, knock_in_level, $level, $($extra,)* observation_dates, effective_date, expiry_date, barrier_state=None, **fields))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only preset terms")]
        pub(crate) fn $name(
            py: Python<'_>,
            $coupon: &Bound<'_, PyAny>,
            initial_spot: &Bound<'_, PyAny>,
            knock_in_level: &Bound<'_, PyAny>,
            $level: &Bound<'_, PyAny>,
            $($extra: &Bound<'_, PyAny>,)*
            observation_dates: &Bound<'_, PyAny>,
            effective_date: &Bound<'_, PyAny>,
            expiry_date: &Bound<'_, PyAny>,
            barrier_state: Option<&Bound<'_, PyAny>>,
            fields: Option<&Bound<'_, PyDict>>,
        ) -> PyResult<SnowballOption> {
            check_changes(fields, &["principal_ratio"])?;
            let mut terms = core::SnowballPresetTerms::new(
                real($coupon)?, real(initial_spot)?, real(knock_in_level)?, real($level)?,
                date(effective_date)?, date(expiry_date)?, dates(observation_dates)?,
            );
            $(let $extra = real($extra)?;)*
            if let Some(value) = barrier_state {
                terms.barrier_state = state(value)?;
            }
            terms.principal_ratio = analytics::real_setting(fields, "principal_ratio", terms.principal_ratio)?;
            Ok(SnowballOption(domain(py, terms.expand($preset))?))
        }
    };
}
snowball_preset!(
    standard_snowball,
    coupon_rate,
    knock_out_level,
    [],
    core::SnowballPreset::Standard
);
snowball_preset!(
    european_snowball,
    coupon_rate,
    knock_out_level,
    [],
    core::SnowballPreset::European
);
snowball_preset!(
    step_down_snowball,
    coupon_rate,
    initial_knock_out_level,
    [knock_out_level_decrement],
    core::SnowballPreset::StepDown {
        knock_out_level_decrement
    }
);
snowball_preset!(
    both_down_snowball,
    initial_coupon_rate,
    initial_knock_out_level,
    [coupon_rate_decrement, knock_out_level_decrement],
    core::SnowballPreset::BothDown {
        coupon_rate_decrement,
        knock_out_level_decrement
    }
);
snowball_preset!(
    dual_coupon_snowball,
    knock_out_coupon_rate,
    knock_out_level,
    [maturity_coupon_rate],
    core::SnowballPreset::DualCoupon {
        maturity_coupon_rate
    }
);
snowball_preset!(
    parachute_snowball,
    coupon_rate,
    knock_out_level,
    [final_knock_out_level],
    core::SnowballPreset::Parachute {
        final_knock_out_level
    }
);
snowball_preset!(
    otm_snowball,
    coupon_rate,
    knock_out_level,
    [upper_strike],
    core::SnowballPreset::Otm { upper_strike }
);
snowball_preset!(
    loss_capped_snowball,
    coupon_rate,
    knock_out_level,
    [lower_strike],
    core::SnowballPreset::LossCapped { lower_strike }
);

/// Owns a validated snowball; replacement never mutates the original.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct SnowballOption(pub core::SnowballOption);
#[pymethods]
impl SnowballOption {
    #[new]
    #[pyo3(signature = (*, maturity_coupon_rate, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation_mode, effective_date, expiry_date, observations, barrier_state=None, **fields))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
        maturity_coupon_rate: &Bound<'_, PyAny>,
        initial_spot: &Bound<'_, PyAny>,
        knock_in_level: &Bound<'_, PyAny>,
        upper_strike: &Bound<'_, PyAny>,
        lower_strike: &Bound<'_, PyAny>,
        knock_in_observation_mode: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        fields: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        check_changes(fields, &["principal_ratio"])?;
        let mut terms = core::SnowballTerms::new(
            real(maturity_coupon_rate)?,
            real(initial_spot)?,
            real(knock_in_level)?,
            real(upper_strike)?,
            real(lower_strike)?,
            date(effective_date)?,
            date(expiry_date)?,
            records(observations)?,
        );
        terms.knock_in_observation_mode = mode(knock_in_observation_mode)?;
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_ratio =
            analytics::real_setting(fields, "principal_ratio", terms.principal_ratio)?;
        Ok(Self(domain(py, core::SnowballOption::new(terms))?))
    }
    #[pyo3(signature = (*, barrier_state))]
    fn replace(&self, py: Python<'_>, barrier_state: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(py, self.0.replace(state(barrier_state)?))?))
    }
    #[getter]
    fn observations(&self) -> Vec<AutocallableObservation> {
        self.0
            .observations()
            .iter()
            .copied()
            .map(AutocallableObservation)
            .collect()
    }
    #[getter]
    fn observation_dates<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDate>>> {
        self.0
            .observations()
            .iter()
            .map(|r| python_date(py, r.date))
            .collect()
    }
    #[getter]
    fn knock_out_levels(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.knock_out_level)
            .collect()
    }
    #[getter]
    fn knock_out_coupon_rates(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.coupon_rate)
            .collect()
    }
    #[getter]
    fn maturity_coupon_rate(&self) -> f64 {
        self.0.terms().maturity_coupon_rate
    }
    #[getter]
    fn initial_spot(&self) -> f64 {
        self.0.terms().initial_spot
    }
    #[getter]
    fn knock_in_level(&self) -> f64 {
        self.0.terms().knock_in_level
    }
    #[getter]
    fn upper_strike(&self) -> f64 {
        self.0.terms().upper_strike
    }
    #[getter]
    fn lower_strike(&self) -> f64 {
        self.0.terms().lower_strike
    }
    #[getter]
    fn principal_ratio(&self) -> f64 {
        self.0.terms().principal_ratio
    }
    #[getter]
    fn knock_in_observation_mode(&self) -> &'static str {
        mode_name(self.0.terms().knock_in_observation_mode)
    }
    #[getter]
    fn barrier_state(&self) -> Option<&'static str> {
        state_name(self.0.barrier_state())
    }
    #[getter]
    fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.effective_date())
    }
    #[getter]
    fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.expiry_date())
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let records = self
            .observations()
            .iter()
            .map(|r| r.__repr__(py))
            .collect::<PyResult<Vec<_>>>()?
            .join(", ");
        let state = self
            .barrier_state()
            .map_or_else(|| "None".into(), |s| format!("{s:?}"));
        Ok(format!(
            "SnowballOption(maturity_coupon_rate={:?}, initial_spot={:?}, knock_in_level={:?}, upper_strike={:?}, lower_strike={:?}, knock_in_observation_mode={:?}, barrier_state={}, principal_ratio={:?}, effective_date={}, expiry_date={}, observations=[{}])",
            self.maturity_coupon_rate(),
            self.initial_spot(),
            self.knock_in_level(),
            self.upper_strike(),
            self.lower_strike(),
            self.knock_in_observation_mode(),
            state,
            self.principal_ratio(),
            self.effective_date(py)?.repr()?,
            self.expiry_date(py)?.repr()?,
            records
        ))
    }
}

/// Owns a validated snowball; replacement never mutates the original.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct BinarySnowballOption(pub core::BinarySnowballOption);
#[pymethods]
impl BinarySnowballOption {
    #[new]
    #[pyo3(signature = (*, maturity_coupon_rate, effective_date, expiry_date, observations, barrier_state=None, **fields))]
    fn new(
        py: Python<'_>,
        maturity_coupon_rate: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        fields: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        check_changes(fields, &["principal_ratio"])?;
        let mut terms = core::BinarySnowballTerms::new(
            real(maturity_coupon_rate)?,
            date(effective_date)?,
            date(expiry_date)?,
            records(observations)?,
        );
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_ratio =
            analytics::real_setting(fields, "principal_ratio", terms.principal_ratio)?;
        Ok(Self(domain(py, core::BinarySnowballOption::new(terms))?))
    }
    #[pyo3(signature = (*, barrier_state))]
    fn replace(&self, py: Python<'_>, barrier_state: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(py, self.0.replace(state(barrier_state)?))?))
    }
    #[getter]
    fn observations(&self) -> Vec<AutocallableObservation> {
        self.0
            .observations()
            .iter()
            .copied()
            .map(AutocallableObservation)
            .collect()
    }
    #[getter]
    fn observation_dates<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDate>>> {
        self.0
            .observations()
            .iter()
            .map(|r| python_date(py, r.date))
            .collect()
    }
    #[getter]
    fn knock_out_levels(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.knock_out_level)
            .collect()
    }
    #[getter]
    fn knock_out_coupon_rates(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.coupon_rate)
            .collect()
    }
    #[getter]
    fn maturity_coupon_rate(&self) -> f64 {
        self.0.terms().maturity_coupon_rate
    }
    #[getter]
    fn principal_ratio(&self) -> f64 {
        self.0.terms().principal_ratio
    }
    #[getter]
    fn barrier_state(&self) -> Option<&'static str> {
        state_name(self.0.barrier_state())
    }
    #[getter]
    fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.effective_date())
    }
    #[getter]
    fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.expiry_date())
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let records = self
            .observations()
            .iter()
            .map(|r| r.__repr__(py))
            .collect::<PyResult<Vec<_>>>()?
            .join(", ");
        let state = self
            .barrier_state()
            .map_or_else(|| "None".into(), |s| format!("{s:?}"));
        Ok(format!(
            "BinarySnowballOption(maturity_coupon_rate={:?}, principal_ratio={:?}, barrier_state={}, effective_date={}, expiry_date={}, observations=[{}])",
            self.maturity_coupon_rate(),
            self.principal_ratio(),
            state,
            self.effective_date(py)?.repr()?,
            self.expiry_date(py)?.repr()?,
            records
        ))
    }
}

/// Owns a validated snowball; replacement never mutates the original.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct TernarySnowballOption(pub core::TernarySnowballOption);
#[pymethods]
impl TernarySnowballOption {
    #[new]
    #[pyo3(signature = (*, maturity_coupon_rate, minimum_coupon_rate, knock_in_level, knock_in_observation_mode, effective_date, expiry_date, observations, barrier_state=None, **fields))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
        maturity_coupon_rate: &Bound<'_, PyAny>,
        minimum_coupon_rate: &Bound<'_, PyAny>,
        knock_in_level: &Bound<'_, PyAny>,
        knock_in_observation_mode: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        fields: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        check_changes(fields, &["principal_ratio"])?;
        let mut terms = core::TernarySnowballTerms::new(
            real(maturity_coupon_rate)?,
            real(minimum_coupon_rate)?,
            real(knock_in_level)?,
            date(effective_date)?,
            date(expiry_date)?,
            records(observations)?,
        );
        terms.knock_in_observation_mode = mode(knock_in_observation_mode)?;
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_ratio =
            analytics::real_setting(fields, "principal_ratio", terms.principal_ratio)?;
        Ok(Self(domain(py, core::TernarySnowballOption::new(terms))?))
    }
    #[pyo3(signature = (*, barrier_state))]
    fn replace(&self, py: Python<'_>, barrier_state: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(py, self.0.replace(state(barrier_state)?))?))
    }
    #[getter]
    fn observations(&self) -> Vec<AutocallableObservation> {
        self.0
            .observations()
            .iter()
            .copied()
            .map(AutocallableObservation)
            .collect()
    }
    #[getter]
    fn observation_dates<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDate>>> {
        self.0
            .observations()
            .iter()
            .map(|r| python_date(py, r.date))
            .collect()
    }
    #[getter]
    fn knock_out_levels(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.knock_out_level)
            .collect()
    }
    #[getter]
    fn knock_out_coupon_rates(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.coupon_rate)
            .collect()
    }
    #[getter]
    fn maturity_coupon_rate(&self) -> f64 {
        self.0.terms().maturity_coupon_rate
    }
    #[getter]
    fn knock_in_level(&self) -> f64 {
        self.0.terms().knock_in_level
    }
    #[getter]
    fn minimum_coupon_rate(&self) -> f64 {
        self.0.terms().minimum_coupon_rate
    }
    #[getter]
    fn principal_ratio(&self) -> f64 {
        self.0.terms().principal_ratio
    }
    #[getter]
    fn knock_in_observation_mode(&self) -> &'static str {
        mode_name(self.0.terms().knock_in_observation_mode)
    }
    #[getter]
    fn barrier_state(&self) -> Option<&'static str> {
        state_name(self.0.barrier_state())
    }
    #[getter]
    fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.effective_date())
    }
    #[getter]
    fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.expiry_date())
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let records = self
            .observations()
            .iter()
            .map(|r| r.__repr__(py))
            .collect::<PyResult<Vec<_>>>()?
            .join(", ");
        let state = self
            .barrier_state()
            .map_or_else(|| "None".into(), |s| format!("{s:?}"));
        Ok(format!(
            "TernarySnowballOption(maturity_coupon_rate={:?}, minimum_coupon_rate={:?}, knock_in_level={:?}, knock_in_observation_mode={:?}, principal_ratio={:?}, barrier_state={}, effective_date={}, expiry_date={}, observations=[{}])",
            self.maturity_coupon_rate(),
            self.minimum_coupon_rate(),
            self.knock_in_level(),
            self.knock_in_observation_mode(),
            self.principal_ratio(),
            state,
            self.effective_date(py)?.repr()?,
            self.expiry_date(py)?.repr()?,
            records
        ))
    }
}

/// Owns a validated Phoenix note; replacement never mutates the original.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct PhoenixOption(pub core::PhoenixOption);
#[pymethods]
impl PhoenixOption {
    #[new]
    #[pyo3(signature = (*, coupon_rate, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation_mode, effective_date, expiry_date, observations, barrier_state=None, **fields))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
        coupon_rate: &Bound<'_, PyAny>,
        initial_spot: &Bound<'_, PyAny>,
        knock_in_level: &Bound<'_, PyAny>,
        upper_strike: &Bound<'_, PyAny>,
        lower_strike: &Bound<'_, PyAny>,
        knock_in_observation_mode: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        fields: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        check_changes(fields, &["principal_ratio"])?;
        let mut terms = core::PhoenixTerms::new(
            real(coupon_rate)?,
            real(initial_spot)?,
            real(knock_in_level)?,
            real(upper_strike)?,
            real(lower_strike)?,
            date(effective_date)?,
            date(expiry_date)?,
            records(observations)?,
        );
        terms.knock_in_observation_mode = mode(knock_in_observation_mode)?;
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_ratio =
            analytics::real_setting(fields, "principal_ratio", terms.principal_ratio)?;
        Ok(Self(domain(py, core::PhoenixOption::new(terms))?))
    }
    #[pyo3(signature = (*, barrier_state))]
    fn replace(&self, py: Python<'_>, barrier_state: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(py, self.0.replace(state(barrier_state)?))?))
    }
    #[getter]
    fn observations(&self) -> Vec<AutocallableObservation> {
        self.0
            .observations()
            .iter()
            .copied()
            .map(AutocallableObservation)
            .collect()
    }
    #[getter]
    fn observation_dates<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDate>>> {
        self.0
            .observations()
            .iter()
            .map(|r| python_date(py, r.date))
            .collect()
    }
    #[getter]
    fn knock_out_levels(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.knock_out_level)
            .collect()
    }
    #[getter]
    fn coupon_barrier_levels(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.coupon_barrier_level)
            .collect()
    }
    #[getter]
    fn coupon_rate(&self) -> f64 {
        self.0.terms().coupon_rate
    }
    #[getter]
    fn initial_spot(&self) -> f64 {
        self.0.terms().initial_spot
    }
    #[getter]
    fn knock_in_level(&self) -> f64 {
        self.0.terms().knock_in_level
    }
    #[getter]
    fn upper_strike(&self) -> f64 {
        self.0.terms().upper_strike
    }
    #[getter]
    fn lower_strike(&self) -> f64 {
        self.0.terms().lower_strike
    }
    #[getter]
    fn principal_ratio(&self) -> f64 {
        self.0.terms().principal_ratio
    }
    #[getter]
    fn knock_in_observation_mode(&self) -> &'static str {
        mode_name(self.0.terms().knock_in_observation_mode)
    }
    #[getter]
    fn barrier_state(&self) -> Option<&'static str> {
        state_name(self.0.barrier_state())
    }
    #[getter]
    fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.effective_date())
    }
    #[getter]
    fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.expiry_date())
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let records = self
            .observations()
            .iter()
            .map(|r| r.__repr__(py))
            .collect::<PyResult<Vec<_>>>()?
            .join(", ");
        let state = self
            .barrier_state()
            .map_or_else(|| "None".into(), |s| format!("{s:?}"));
        Ok(format!(
            "PhoenixOption(coupon_rate={:?}, initial_spot={:?}, knock_in_level={:?}, upper_strike={:?}, lower_strike={:?}, knock_in_observation_mode={:?}, barrier_state={}, principal_ratio={:?}, effective_date={}, expiry_date={}, observations=[{}])",
            self.coupon_rate(),
            self.initial_spot(),
            self.knock_in_level(),
            self.upper_strike(),
            self.lower_strike(),
            self.knock_in_observation_mode(),
            state,
            self.principal_ratio(),
            self.effective_date(py)?.repr()?,
            self.expiry_date(py)?.repr()?,
            records
        ))
    }
}

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct QuadratureAutocallableEngine(pub(crate) core::QuadratureAutocallableEngine);

#[pymethods]
impl QuadratureAutocallableEngine {
    #[new]
    #[pyo3(signature = (**fields))]
    fn new(py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(
            fields,
            &["asset_step_count", "integration_standard_deviations"],
        )?;
        let mut settings = core::QuadratureSettings::default();
        if let Some(value) = fields
            .map(|f| f.get_item("asset_step_count"))
            .transpose()?
            .flatten()
        {
            settings.asset_step_count = integer(&value)?;
        }
        if let Some(value) = fields
            .map(|f| f.get_item("integration_standard_deviations"))
            .transpose()?
            .flatten()
        {
            settings.integration_standard_deviations = real(&value)?;
        }
        Ok(Self(domain(
            py,
            core::QuadratureAutocallableEngine::new(settings),
        )?))
    }
    #[getter]
    fn asset_step_count(&self) -> i32 {
        self.0.settings().asset_step_count
    }
    #[getter]
    fn integration_standard_deviations(&self) -> f64 {
        self.0.settings().integration_standard_deviations
    }
    fn __repr__(&self) -> String {
        format!(
            "QuadratureAutocallableEngine(asset_step_count={}, integration_standard_deviations={})",
            self.asset_step_count(),
            self.integration_standard_deviations()
        )
    }
    fn price(
        &self,
        py: Python<'_>,
        instrument: &Bound<'_, PyAny>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let context = context.0.clone();
        if let Ok(note) = instrument.extract::<PyRef<'_, PhoenixOption>>() {
            let option = note.0.clone();
            return domain(py, py.detach(|| engine.price(&option, &context)));
        }
        if let Ok(note) = instrument.extract::<PyRef<'_, BinarySnowballOption>>() {
            let option = note.0.clone();
            return domain(py, py.detach(|| engine.price(&option, &context)));
        }
        if let Ok(note) = instrument.extract::<PyRef<'_, TernarySnowballOption>>() {
            let option = note.0.clone();
            return domain(py, py.detach(|| engine.price(&option, &context)));
        }
        let option = instrument.extract::<PyRef<'_, SnowballOption>>()?.0.clone();
        domain(py, py.detach(|| engine.price(&option, &context)))
    }
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: &Bound<'_, PyAny>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<analytics::PricingResult> {
        let request = analytics::request(greeks, settings)?;
        let shifts = analytics::shifts(settings)?;
        let engine = self.0;
        let context = context.0.clone();
        if let Ok(note) = instrument.extract::<PyRef<'_, PhoenixOption>>() {
            let option = note.0.clone();
            return Ok(analytics::PricingResult(domain(
                py,
                py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
            )?));
        }
        if let Ok(note) = instrument.extract::<PyRef<'_, BinarySnowballOption>>() {
            let option = note.0.clone();
            return Ok(analytics::PricingResult(domain(
                py,
                py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
            )?));
        }
        if let Ok(note) = instrument.extract::<PyRef<'_, TernarySnowballOption>>() {
            let option = note.0.clone();
            return Ok(analytics::PricingResult(domain(
                py,
                py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
            )?));
        }
        let option = instrument.extract::<PyRef<'_, SnowballOption>>()?.0.clone();
        Ok(analytics::PricingResult(domain(
            py,
            py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
        )?))
    }
}
