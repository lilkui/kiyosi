#include "_binding.hpp"

#include <array>

using namespace nb::literals;

namespace kiyosi::python_binding {

namespace {

inline constexpr std::array<std::pair<const char*, RiskMeasure>, risk_measure_count> risk_measures{{
    {"price", RiskMeasure::price}, {"delta", RiskMeasure::delta},
    {"gamma", RiskMeasure::gamma}, {"speed", RiskMeasure::speed},
    {"theta", RiskMeasure::theta}, {"charm", RiskMeasure::charm},
    {"color", RiskMeasure::color}, {"vega", RiskMeasure::vega},
    {"vanna", RiskMeasure::vanna}, {"zomma", RiskMeasure::zomma},
    {"rho", RiskMeasure::rho},
}};

std::optional<RiskMeasure> measure_named(std::string_view name)
{
    for (const auto& [candidate, measure] : risk_measures)
        if (name == candidate) return measure;
    return std::nullopt;
}

MonteCarloBackend monte_carlo_backend_value(nb::handle value)
{
    if (!nb::isinstance<MonteCarloBackend>(value))
        type_error("backend", "a MonteCarloBackend");
    return nb::cast<MonteCarloBackend>(value);
}

PythonOptionalReal optional_value(const PricingResult& result, RiskMeasure measure)
{
    const auto value = result.get(measure);
    if (value && *value) return PythonOptionalReal{nb::float_(**value)};
    return PythonOptionalReal{nb::none()};
}

nb::tuple result_keys()
{
    nb::tuple output = nb::steal<nb::tuple>(PyTuple_New(risk_measure_count));
    for (std::size_t index = 0; index < risk_measures.size(); ++index) {
        nb::object value = nb::str(risk_measures[index].first);
        PyTuple_SET_ITEM(output.ptr(), index, value.release().ptr());
    }
    return output;
}

nb::tuple result_values(const PricingResult& result)
{
    nb::tuple output = nb::steal<nb::tuple>(PyTuple_New(risk_measure_count));
    for (std::size_t index = 0; index < risk_measures.size(); ++index) {
        nb::object value = optional_value(result, risk_measures[index].second);
        PyTuple_SET_ITEM(output.ptr(), index, value.release().ptr());
    }
    return output;
}

nb::tuple result_items(const PricingResult& result)
{
    nb::tuple output = nb::steal<nb::tuple>(PyTuple_New(risk_measure_count));
    for (std::size_t index = 0; index < risk_measures.size(); ++index) {
        nb::object value = nb::make_tuple(risk_measures[index].first,
                                          optional_value(result, risk_measures[index].second));
        PyTuple_SET_ITEM(output.ptr(), index, value.release().ptr());
    }
    return output;
}

NumericalShiftSettings numerical_settings(
    nb::handle spot_shift, nb::handle volatility_shift, nb::handle rate_shift,
    nb::handle time_shift_days);

template <typename Engine, typename Instrument>
void bind_engine_price(nb::class_<Engine>& binding)
{
    binding.def(
        "price",
        [](const Engine& engine, const Instrument& instrument, const PricingContext& context) {
            return unwrap(engine.price(instrument, context));
        },
        "instrument"_a, "context"_a, nb::call_guard<nb::gil_scoped_release>(),
        R"doc(Price an instrument under a market context.

Parameters
----------
instrument : instrument
    Instrument accepted by this overload.
context : PricingContext
    Market state and valuation instant.

Returns
-------
float
    Instrument price. No Greeks are calculated.

Raises
------
KiyosiError
    If instrument terms, context, engine settings, backend availability, or
    the requested valuation are invalid.)doc");
    binding.def(
        "price_with_greeks",
        [](const Engine& engine, const Instrument& instrument, const PricingContext& context,
           GreeksLevel level, PythonReal spot_shift, PythonReal volatility_shift,
           PythonReal rate_shift, PythonInteger time_shift_days) {
            const auto settings = numerical_settings(
                spot_shift, volatility_shift, rate_shift, time_shift_days);
            nb::gil_scoped_release release;
            return unwrap(engine.price_with_greeks(instrument, context, level, settings));
        },
        "instrument"_a, "context"_a, "level"_a.noconvert(), nb::kw_only(),
        "spot_shift"_a = NumericalShiftSettings{}.spot_shift,
        "volatility_shift"_a = NumericalShiftSettings{}.volatility_shift,
        "rate_shift"_a = NumericalShiftSettings{}.rate_shift,
        "time_shift_days"_a = NumericalShiftSettings{}.time_shift_days,
        R"doc(Compute price and the explicitly selected Greeks tier.

``GreeksLevel.basic`` requests delta and gamma; ``GreeksLevel.full`` requests
all ten Greeks. Native values are reused and missing feasible measures use
numerical price differences. Unrequested or undefined measures are None, never
zero sentinels. At expiry or a monitored barrier hit-state boundary, only price
is available. Feasible bumped valuation failures fail the entire operation.
At current structured-product event thresholds, undefined spot and time
sensitivities remain None while feasible rate and volatility sensitivities survive.

Shift keyword arguments are absolute, use core-owned defaults, and follow
calculate_numerical_risk_measures. Monte Carlo valuations share one seed per
request without changing the engine. Concurrent calls are safe.

Returns
-------
PricingResult
    Price and requested available Greeks, with RiskMeasure units.

Raises
------
TypeError
    If level, engine/instrument pairing, or a shift representation is incompatible.
OverflowError
    If time_shift_days is outside the C++ int range.
KiyosiError
    If settings are invalid or a required valuation fails.)doc");
}

template <typename Engine>
nb::class_<Engine> bind_stateless_engine(nb::module_& module, const char* name)
{
    nb::class_<Engine> binding{
        module, name,
        R"doc(Stateless pricing engine.

The engine has no configuration and may be reused across supported instruments
and contexts. Concurrent calls are safe.)doc"};
    binding.def(nb::init<>(), R"doc(Create a stateless pricing engine.

Returns
-------
pricing engine
    Reusable engine with no mutable configuration.)doc");
    bind_repr(binding, name, {});
    return binding;
}

template <typename Engine>
nb::class_<Engine> bind_finite_difference_engine(nb::module_& module, const char* name)
{
    nb::class_<Engine> binding{
        module, name,
        R"doc(Finite-difference pricing engine with immutable configuration.

Settings are stored without domain validation and are validated when price() is called.

Attributes
----------
asset_step_count : int
    Number of spatial grid steps.
time_step_count : int
    Number of time grid steps.
scheme : FiniteDifferenceScheme
    Time-stepping scheme.
asset_upper_boundary : float or None
    Explicit upper asset-grid boundary, or ``None`` for the core default.)doc"};
    binding
        .def(nb::new_([](PythonInteger asset_step_count, PythonInteger time_step_count,
                        FiniteDifferenceScheme scheme, PythonReal asset_upper_boundary) {
                 std::optional<double> boundary;
                 if (!asset_upper_boundary.is_none())
                     boundary = real_number(asset_upper_boundary, "asset_upper_boundary");
                 return Engine{FiniteDifferenceSettings{
                     integer(asset_step_count, "asset_step_count"), integer(time_step_count, "time_step_count"),
                     scheme, boundary}};
             }),
             nb::kw_only(), "asset_step_count"_a = FiniteDifferenceSettings{}.asset_step_count,
             "time_step_count"_a = FiniteDifferenceSettings{}.time_step_count,
             "scheme"_a = FiniteDifferenceSettings{}.scheme,
             "asset_upper_boundary"_a = nb::none(),
             R"doc(Store finite-difference settings.

Settings are validated when price() is called.

Parameters
----------
asset_step_count : int, optional
    Number of spatial grid steps. Uses the core default when omitted.
time_step_count : int, optional
    Number of time grid steps. Uses the core default when omitted.
scheme : FiniteDifferenceScheme, optional
    Time-stepping scheme. Uses the core default when omitted.
asset_upper_boundary : float or None, optional
    Explicit upper asset-grid boundary, or ``None`` for the core default.

Raises
------
TypeError
    If a setting has an incompatible representation.
OverflowError
    If an integer setting is outside the C++ ``int`` range.)doc")
        .def_prop_ro("asset_step_count", [](const Engine& engine) { return engine.settings().asset_step_count; },
                     "Number of spatial grid steps.")
        .def_prop_ro("time_step_count", [](const Engine& engine) { return engine.settings().time_step_count; },
                     "Number of time grid steps.")
        .def_prop_ro("scheme", [](const Engine& engine) { return engine.settings().scheme; },
                     "Finite-difference time-stepping scheme.")
        .def_prop_ro("asset_upper_boundary", [](const Engine& engine) { return engine.settings().asset_upper_boundary; },
                     "Explicit upper asset-grid boundary, or None for the core default.");
    bind_repr(binding, name,
              {{"asset_step_count", "asset_step_count"}, {"time_step_count", "time_step_count"},
               {"scheme", "scheme"}, {"asset_upper_boundary", "asset_upper_boundary"}});
    return binding;
}

template <typename Engine>
nb::class_<Engine> bind_structured_monte_carlo_engine(nb::module_& module, const char* name)
{
    nb::class_<Engine> binding{
        module, name,
        R"doc(Trading-day Monte Carlo engine with immutable configuration.

Settings are stored without domain validation and are validated when price() is called.

Attributes
----------
path_count : int
    Number of simulated paths.
seed : int or None
    Non-negative random seed.
backend : MonteCarloBackend
    CPU or CUDA execution backend.)doc"};
    binding
        .def(nb::new_([](PythonInteger path_count, PythonInteger seed, nb::handle backend) {
                 return Engine{TradingDayMonteCarloSettings{
                     integer(path_count, "path_count"), optional_seed(seed),
                     monte_carlo_backend_value(backend)}};
             }),
             nb::kw_only(), "path_count"_a = TradingDayMonteCarloSettings{}.path_count,
             "seed"_a = TradingDayMonteCarloSettings{}.seed.value(),
             "backend"_a = TradingDayMonteCarloSettings{}.backend,
             R"doc(Store Monte Carlo settings.

Settings are validated when price() is called.

Parameters
----------
path_count : int, optional
    Number of simulated paths. Uses the core default when omitted.
seed : int or None, optional
    Non-negative random seed. Uses the core default when omitted.
backend : MonteCarloBackend, optional
    CPU or CUDA execution backend.

Raises
------
TypeError
    If a setting has an incompatible representation.
OverflowError
    If an integer is outside its C++ representation range.)doc")
        .def_prop_ro("path_count", [](const Engine& engine) { return engine.settings().path_count; },
                     "Number of simulated paths.")
        .def_prop_ro("seed", [](const Engine& engine) { return engine.settings().seed; },
                     "Non-negative random seed.")
        .def_prop_ro("backend", [](const Engine& engine) { return engine.settings().backend; },
                     "CPU or CUDA execution backend.");
    bind_repr(binding, name,
              {{"path_count", "path_count"}, {"seed", "seed"}, {"backend", "backend"}});
    return binding;
}

NumericalShiftSettings numerical_settings(
    nb::handle spot_shift, nb::handle volatility_shift, nb::handle rate_shift,
    nb::handle time_shift_days)
{
    return {real_number(spot_shift, "spot_shift"),
            real_number(volatility_shift, "volatility_shift"),
            real_number(rate_shift, "rate_shift"), integer(time_shift_days, "time_shift_days")};
}

ImpliedVolatilitySettings volatility_settings(
    nb::handle lower_bound, nb::handle upper_bound, nb::handle tolerance,
    nb::handle max_iterations)
{
    return {real_number(lower_bound, "lower_bound"),
            real_number(upper_bound, "upper_bound"), real_number(tolerance, "tolerance"),
            integer(max_iterations, "max_iterations")};
}

ImpliedCouponSettings coupon_settings(
    nb::handle lower_bound, nb::handle upper_bound, nb::handle tolerance,
    nb::handle max_iterations)
{
    return {real_number(lower_bound, "lower_bound"),
            real_number(upper_bound, "upper_bound"), real_number(tolerance, "tolerance"),
            integer(max_iterations, "max_iterations")};
}

template <typename Engine, typename Instrument>
void bind_analytics_pair(nb::module_& module)
{
    module.def(
        "calculate_numerical_risk_measures",
        [](const Engine& engine, const Instrument& instrument, const PricingContext& context,
           PythonReal spot_shift, PythonReal volatility_shift, PythonReal rate_shift,
           PythonInteger time_shift_days) {
            const auto settings = numerical_settings(
                spot_shift, volatility_shift, rate_shift, time_shift_days);
            nb::gil_scoped_release release;
            return unwrap(kiyosi::calculate_numerical_risk_measures(engine, instrument, context, settings));
        },
        "engine"_a, "instrument"_a, "context"_a, nb::kw_only(),
        "spot_shift"_a = NumericalShiftSettings{}.spot_shift,
        "volatility_shift"_a = NumericalShiftSettings{}.volatility_shift,
        "rate_shift"_a = NumericalShiftSettings{}.rate_shift,
        "time_shift_days"_a = NumericalShiftSettings{}.time_shift_days,
        R"doc(Compute price and every feasible numerical risk measure.

Shifts are absolute. Omitted shifts use core-owned defaults. A measure with no
valid finite-difference stencil inside a model boundary is ``None`` while other
valid results are preserved.

Parameters
----------
engine : pricing engine
    Engine used for every base and bumped valuation.
instrument : instrument
    Instrument supported by the engine.
context : PricingContext
    Market state and valuation instant.
spot_shift : float, optional
    Absolute spot change for spot-based sensitivities.
volatility_shift : float, optional
    Absolute volatility change for volatility-based sensitivities.
rate_shift : float, optional
    Absolute risk-free-rate change for rate-based sensitivities.
time_shift_days : int, optional
    Calendar-day step for time-based sensitivities.

Returns
-------
PricingResult
    Price and each feasible numerical sensitivity.

Raises
------
TypeError
    If the engine/instrument combination or a setting is incompatible.
KiyosiError
    If the inputs are invalid or a feasible valuation fails.)doc");
    module.def(
        "implied_volatility",
        [](const Engine& engine, const Instrument& instrument, const PricingContext& context,
           PythonReal observed_price, PythonReal lower_bound, PythonReal upper_bound,
           PythonReal tolerance, PythonInteger max_iterations) {
            const double observed = real_number(observed_price, "observed_price");
            const auto settings = volatility_settings(
                lower_bound, upper_bound, tolerance, max_iterations);
            nb::gil_scoped_release release;
            return unwrap(kiyosi::implied_volatility(
                engine, instrument, context, observed, settings));
        },
        "engine"_a, "instrument"_a, "context"_a, "observed_price"_a, nb::kw_only(),
        "lower_bound"_a = ImpliedVolatilitySettings{}.lower_bound,
        "upper_bound"_a = ImpliedVolatilitySettings{}.upper_bound,
        "tolerance"_a = ImpliedVolatilitySettings{}.tolerance,
        "max_iterations"_a = ImpliedVolatilitySettings{}.max_iterations,
        R"doc(Solve for the volatility that matches an observed price.

Parameters
----------
engine : pricing engine
    Engine used for trial valuations.
instrument : instrument
    Instrument supported by the engine.
context : PricingContext
    Market state whose volatility is varied.
observed_price : float
    Target instrument price.
lower_bound, upper_bound : float, optional
    Volatility search interval. Omitted values use core defaults.
tolerance : float, optional
    Solver convergence tolerance. With Monte Carlo, this applies to the sampled price curve, not sampling error.
max_iterations : int, optional
    Maximum solver iterations.

Returns
-------
float
    Implied volatility as a decimal rate.

An unseeded Monte Carlo engine uses one random seed throughout this solve.

Raises
------
TypeError
    If the engine/instrument combination or an argument is incompatible.
KiyosiError
    If inputs are invalid, the target is not bracketed, or the solver fails.)doc");
}

template <typename Engine, typename Instrument>
void bind_implied_coupon_pair(nb::module_& module)
{
    module.def(
        "implied_coupon",
        [](const Engine& engine, const Instrument& instrument, const PricingContext& context,
           PythonReal observed_price, PythonReal lower_bound, PythonReal upper_bound,
           PythonReal tolerance, PythonInteger max_iterations) {
            const double observed = real_number(observed_price, "observed_price");
            const auto settings = coupon_settings(
                lower_bound, upper_bound, tolerance, max_iterations);
            nb::gil_scoped_release release;
            return unwrap(kiyosi::implied_coupon(
                engine, instrument, context, observed, settings));
        },
        "engine"_a, "instrument"_a, "context"_a, "observed_price"_a, nb::kw_only(),
        "lower_bound"_a = ImpliedCouponSettings{}.lower_bound,
        "upper_bound"_a = ImpliedCouponSettings{}.upper_bound,
        "tolerance"_a = ImpliedCouponSettings{}.tolerance,
        "max_iterations"_a = ImpliedCouponSettings{}.max_iterations,
        R"doc(Solve for a Phoenix coupon rate that matches an observed price.

Parameters
----------
engine : pricing engine
    Engine used for trial valuations.
instrument : PhoenixOption
    Phoenix option supported by the engine.
context : PricingContext
    Market state used by the solver.
observed_price : float
    Target instrument price.
lower_bound, upper_bound : float, optional
    Finite coupon-rate search interval, which may include negative rates. Omitted values use core defaults.
tolerance : float, optional
    Solver convergence tolerance. With Monte Carlo, this applies to the sampled price curve, not sampling error.
max_iterations : int, optional
    Maximum solver iterations.

Returns
-------
float
    Implied annualized coupon rate.

An unseeded Monte Carlo engine uses one random seed throughout this solve.

Raises
------
TypeError
    If the engine/instrument combination or an argument is incompatible.
KiyosiError
    If inputs are invalid, the target is not bracketed, or the solver fails.)doc");
}

template <typename Engine, typename Instrument>
void bind_snowball_implied_coupon_pair(nb::module_& module)
{
    module.def(
        "implied_coupon",
        [](const Engine& engine, const Instrument& instrument, const PricingContext& context,
           PythonReal observed_price, CouponQuoteConvention quote_convention,
           PythonReal lower_bound, PythonReal upper_bound, PythonReal tolerance,
           PythonInteger max_iterations) {
            const double observed = real_number(observed_price, "observed_price");
            const auto settings = coupon_settings(
                lower_bound, upper_bound, tolerance, max_iterations);
            nb::gil_scoped_release release;
            return unwrap(kiyosi::implied_coupon(
                engine, instrument, context, observed, quote_convention, settings));
        },
        "engine"_a, "instrument"_a, "context"_a, "observed_price"_a, nb::kw_only(),
        "quote_convention"_a,
        "lower_bound"_a = ImpliedCouponSettings{}.lower_bound,
        "upper_bound"_a = ImpliedCouponSettings{}.upper_bound,
        "tolerance"_a = ImpliedCouponSettings{}.tolerance,
        "max_iterations"_a = ImpliedCouponSettings{}.max_iterations,
        R"doc(Solve for a snowball coupon rate that matches an observed price.

Parameters
----------
engine : pricing engine
    Engine used for trial valuations.
instrument : SnowballOption, BinarySnowballOption, or TernarySnowballOption
    Snowball instrument supported by the engine.
context : PricingContext
    Market state used by the solver.
observed_price : float
    Target instrument price.
quote_convention : CouponQuoteConvention
    Whether the maturity coupon shifts with quoted knock-out coupons.
lower_bound, upper_bound : float, optional
    Finite coupon-rate search interval, which may include negative rates. Omitted values use core defaults.
tolerance : float, optional
    Solver convergence tolerance. With Monte Carlo, this applies to the sampled price curve, not sampling error.
max_iterations : int, optional
    Maximum solver iterations.

Returns
-------
float
    Implied annualized quoted coupon rate.

An unseeded Monte Carlo engine uses one random seed throughout this solve.

Raises
------
TypeError
    If the engine/instrument combination or an argument is incompatible.
KiyosiError
    If inputs are invalid, the target is not bracketed, or the solver fails.)doc");
}

template <typename Engine, typename... Instruments>
void bind_engine_analytics(nb::module_& module)
{
    (bind_analytics_pair<Engine, Instruments>(module), ...);
}

} // namespace

void bind_results(nb::module_& module)
{
    nb::enum_<GreeksLevel>(module, "GreeksLevel",
        "Explicit Greeks calculation tier; basic is delta/gamma, full is all ten Greeks.")
        .value("basic", GreeksLevel::basic)
        .value("full", GreeksLevel::full);

    auto pricing_result = nb::class_<PricingResult>(
        module, "PricingResult",
        R"doc(Read-only mapping over the fixed risk-measure vocabulary.

Price uses instrument value units. Delta, gamma, and speed are per one spot unit,
squared spot unit, and cubed spot unit. Vega, vanna, and zomma are price, delta,
and gamma changes per one volatility percentage point (an absolute change of
0.01). Rho is per one interest-rate percentage point. Theta, charm, and color
are price, delta, and gamma changes per calendar day as valuation time moves
forward. Undefined or unsupported measures are None, never a zero sentinel.

Attributes
----------
price, delta, gamma, speed : float or None
    Price and first three spot derivatives.
theta, charm, color : float or None
    Daily changes in price, delta, and gamma.
vega, vanna, zomma : float or None
    Volatility-point changes in price, delta, and gamma.
rho : float or None
    Price change per interest-rate percentage point.)doc")
        .def("__len__", [](const PricingResult&) { return risk_measure_count; })
        .def("__iter__", [](const PricingResult&) {
            return PythonStringIterator{result_keys().attr("__iter__")()};
        })
        .def("__contains__", [](const PricingResult&, nb::handle key) {
            return nb::isinstance<nb::str>(key) &&
                   measure_named(nb::cast<std::string>(key)).has_value();
        }, nb::arg().none())
        .def("__getitem__", [](const PricingResult& result, nb::str key) {
            const std::string name = nb::cast<std::string>(key);
            const auto measure = measure_named(name);
            if (!measure) throw nb::key_error(name.c_str());
            return optional_value(result, *measure);
        })
        .def("get", [](const PricingResult& result, nb::str key, nb::object fallback) {
            const auto measure = measure_named(nb::cast<std::string>(key));
            return measure ? optional_value(result, *measure) : fallback;
        }, "key"_a, "default"_a = nb::none(), R"doc(Return a measure by name.

Parameters
----------
key : str
    Lowercase risk-measure name.
default : object, optional
    Value returned when ``key`` is unknown.

Returns
-------
float, None, or object
    Measure value, ``None`` when unavailable, or ``default`` for an unknown key.)doc")
        .def("keys", [](const PricingResult&) { return result_keys(); },
             R"doc(Return risk-measure names in stable order.

Returns
-------
tuple[str, ...]
    Fixed lowercase risk-measure names.)doc")
        .def("values", [](const PricingResult& result) { return result_values(result); },
             R"doc(Return risk-measure values in stable order.

Returns
-------
tuple[float or None, ...]
    Values aligned with :meth:`keys`.)doc")
        .def("items", [](const PricingResult& result) { return result_items(result); },
             R"doc(Return name-value pairs in stable order.

Returns
-------
tuple[tuple[str, float or None], ...]
    Pairs aligned with :meth:`keys`.)doc")
        .def("require", [](const PricingResult& result, RiskMeasure measure) {
            return unwrap(result.require(measure));
        }, "measure"_a, R"doc(Return a required risk measure.

Parameters
----------
measure : RiskMeasure
    Measure to retrieve.

Returns
-------
float
    Available measure value.

Raises
------
KiyosiError
    If the requested measure is unavailable.)doc")
        .def_prop_ro("price", [](const PricingResult& result) {
            return optional_value(result, RiskMeasure::price);
        }, "Instrument value, or None when unavailable.")
        .def_prop_ro("delta", [](const PricingResult& result) {
            return optional_value(result, RiskMeasure::delta);
        }, "First spot derivative, or None when unavailable.")
        .def_prop_ro("gamma", [](const PricingResult& result) {
            return optional_value(result, RiskMeasure::gamma);
        }, "Second spot derivative, or None when unavailable.")
        .def_prop_ro("speed", [](const PricingResult& result) {
            return optional_value(result, RiskMeasure::speed);
        }, "Third spot derivative, or None when unavailable.")
        .def_prop_ro("theta", [](const PricingResult& result) {
            return optional_value(result, RiskMeasure::theta);
        }, "Daily price decay, or None when unavailable.")
        .def_prop_ro("charm", [](const PricingResult& result) {
            return optional_value(result, RiskMeasure::charm);
        }, "Daily change in delta, or None when unavailable.")
        .def_prop_ro("color", [](const PricingResult& result) {
            return optional_value(result, RiskMeasure::color);
        }, "Daily change in gamma, or None when unavailable.")
        .def_prop_ro("vega", [](const PricingResult& result) {
            return optional_value(result, RiskMeasure::vega);
        }, "Price change per volatility percentage point, or None.")
        .def_prop_ro("vanna", [](const PricingResult& result) {
            return optional_value(result, RiskMeasure::vanna);
        }, "Delta change per volatility percentage point, or None.")
        .def_prop_ro("zomma", [](const PricingResult& result) {
            return optional_value(result, RiskMeasure::zomma);
        }, "Gamma change per volatility percentage point, or None.")
        .def_prop_ro("rho", [](const PricingResult& result) {
            return optional_value(result, RiskMeasure::rho);
        }, "Price change per interest-rate percentage point, or None.");
    bind_repr(pricing_result, "PricingResult",
              {{"price", "price"}, {"delta", "delta"}, {"gamma", "gamma"},
               {"speed", "speed"}, {"theta", "theta"}, {"charm", "charm"},
               {"color", "color"}, {"vega", "vega"}, {"vanna", "vanna"},
               {"zomma", "zomma"}, {"rho", "rho"}});
    nb::module_::import_("collections.abc").attr("Mapping").attr("register")(
        module.attr("PricingResult"));
}

void bind_engines(nb::module_& module)
{
    auto analytic_vanilla = bind_stateless_engine<AnalyticVanillaEngine>(
        module, "AnalyticVanillaEngine");
    bind_engine_price<AnalyticVanillaEngine, EuropeanOption>(analytic_vanilla);
    auto integral_vanilla = bind_stateless_engine<QuadratureVanillaEngine>(
        module, "QuadratureVanillaEngine");
    bind_engine_price<QuadratureVanillaEngine, EuropeanOption>(integral_vanilla);
    auto crr = nb::class_<CoxRossRubinsteinVanillaEngine>(
        module, "CoxRossRubinsteinVanillaEngine",
        R"doc(Cox-Ross-Rubinstein binomial vanilla-option engine.

Configuration is immutable. Settings are stored without domain validation and
are validated when price() is called.

Attributes
----------
step_count : int
    Number of binomial time steps.)doc")
        .def(nb::new_([](PythonInteger step_count) {
                 return CoxRossRubinsteinVanillaEngine{integer(step_count, "step_count")};
             }),
             "step_count"_a = BinomialSettings{}.step_count,
             R"doc(Store Cox-Ross-Rubinstein settings.

Parameters
----------
step_count : int, optional
    Number of binomial time steps. Uses the core default when omitted.

Raises
------
TypeError
    If ``step_count`` is not an integer.
OverflowError
    If ``step_count`` is outside the C++ ``int`` range.

Notes
-----
The setting is validated when price() is called.)doc")
        .def_prop_ro("step_count", [](const CoxRossRubinsteinVanillaEngine& engine) {
            return engine.settings().step_count;
        }, "Number of binomial time steps.");
    bind_repr(crr, "CoxRossRubinsteinVanillaEngine", {{"step_count", "step_count"}});
    bind_engine_price<CoxRossRubinsteinVanillaEngine, EuropeanOption>(crr);
    bind_engine_price<CoxRossRubinsteinVanillaEngine, AmericanOption>(crr);
    auto bjerksund = bind_stateless_engine<BjerksundStenslandVanillaEngine>(
        module, "BjerksundStenslandVanillaEngine");
    bind_engine_price<BjerksundStenslandVanillaEngine, AmericanOption>(bjerksund);
    auto finite_vanilla = bind_finite_difference_engine<FiniteDifferenceVanillaEngine>(
        module, "FiniteDifferenceVanillaEngine");
    bind_engine_price<FiniteDifferenceVanillaEngine, EuropeanOption>(finite_vanilla);
    bind_engine_price<FiniteDifferenceVanillaEngine, AmericanOption>(finite_vanilla);
    auto monte_carlo_vanilla = nb::class_<MonteCarloVanillaEngine>(
        module, "MonteCarloVanillaEngine",
        R"doc(Monte Carlo vanilla-option engine with immutable configuration.

Settings are stored without domain validation and are validated when price() is called.

Attributes
----------
path_count : int
    Number of simulated paths.
step_count : int
    Number of time steps per path.
seed : int or None
    Optional non-negative random seed.
backend : MonteCarloBackend
    CPU or CUDA execution backend.)doc")
        .def(nb::new_([](PythonInteger path_count, PythonInteger step_count,
                        PythonInteger seed, nb::handle backend) {
                 return MonteCarloVanillaEngine{MonteCarloSettings{
                     integer(path_count, "path_count"), integer(step_count, "step_count"),
                     optional_seed(seed), monte_carlo_backend_value(backend)}};
             }),
             nb::kw_only(), "path_count"_a = MonteCarloSettings{}.path_count,
             "step_count"_a = MonteCarloSettings{}.step_count,
             "seed"_a = nb::none(),
             "backend"_a = MonteCarloSettings{}.backend,
             R"doc(Store Monte Carlo settings.

Parameters
----------
path_count : int, optional
    Number of simulated paths. Uses the core default when omitted.
step_count : int, optional
    Number of time steps per path. Uses the core default when omitted.
seed : int or None, optional
    Non-negative random seed, or ``None`` for nondeterministic seeding.
backend : MonteCarloBackend, optional
    CPU or CUDA execution backend.

Raises
------
TypeError
    If a setting has an incompatible representation.
OverflowError
    If an integer is outside its C++ representation range.

Notes
-----
Settings are validated when price() is called.)doc")
        .def_prop_ro("path_count", [](const MonteCarloVanillaEngine& engine) {
            return engine.settings().path_count;
        }, "Number of simulated paths.")
        .def_prop_ro("step_count", [](const MonteCarloVanillaEngine& engine) {
            return engine.settings().step_count;
        }, "Number of time steps per path.")
        .def_prop_ro("seed", [](const MonteCarloVanillaEngine& engine) {
            return engine.settings().seed;
        }, "Optional non-negative random seed.")
        .def_prop_ro("backend", [](const MonteCarloVanillaEngine& engine) {
            return engine.settings().backend;
        }, "CPU or CUDA execution backend.");
    bind_repr(monte_carlo_vanilla, "MonteCarloVanillaEngine",
              {{"path_count", "path_count"}, {"step_count", "step_count"},
               {"seed", "seed"}, {"backend", "backend"}});
    bind_engine_price<MonteCarloVanillaEngine, EuropeanOption>(monte_carlo_vanilla);
    bind_engine_price<MonteCarloVanillaEngine, AmericanOption>(monte_carlo_vanilla);

    auto analytic_digital = bind_stateless_engine<AnalyticDigitalEngine>(
        module, "AnalyticDigitalEngine");
    bind_engine_price<AnalyticDigitalEngine, CashOrNothingOption>(analytic_digital);
    bind_engine_price<AnalyticDigitalEngine, AssetOrNothingOption>(analytic_digital);
    auto integral_digital = bind_stateless_engine<QuadratureDigitalEngine>(
        module, "QuadratureDigitalEngine");
    bind_engine_price<QuadratureDigitalEngine, CashOrNothingOption>(integral_digital);
    bind_engine_price<QuadratureDigitalEngine, AssetOrNothingOption>(integral_digital);
    auto finite_digital = bind_finite_difference_engine<FiniteDifferenceDigitalEngine>(
        module, "FiniteDifferenceDigitalEngine");
    bind_engine_price<FiniteDifferenceDigitalEngine, CashOrNothingOption>(finite_digital);
    bind_engine_price<FiniteDifferenceDigitalEngine, AssetOrNothingOption>(finite_digital);

    auto analytic_barrier = bind_stateless_engine<AnalyticBarrierEngine>(
        module, "AnalyticBarrierEngine");
    bind_engine_price<AnalyticBarrierEngine, BarrierOption>(analytic_barrier);
    auto finite_barrier = bind_finite_difference_engine<FiniteDifferenceBarrierEngine>(
        module, "FiniteDifferenceBarrierEngine");
    bind_engine_price<FiniteDifferenceBarrierEngine, BarrierOption>(finite_barrier);
    auto analytic_binary = bind_stateless_engine<AnalyticBinaryBarrierEngine>(
        module, "AnalyticBinaryBarrierEngine");
    bind_engine_price<AnalyticBinaryBarrierEngine, BinaryBarrierOption>(analytic_binary);
    bind_engine_price<AnalyticBinaryBarrierEngine, TouchOption>(analytic_binary);

    auto geometric = bind_stateless_engine<AnalyticGeometricAveragePriceEngine>(
        module, "AnalyticGeometricAveragePriceEngine");
    bind_engine_price<AnalyticGeometricAveragePriceEngine, GeometricAveragePriceOption>(geometric);
    auto arithmetic = bind_stateless_engine<TurnbullWakemanArithmeticAveragePriceEngine>(
        module, "TurnbullWakemanArithmeticAveragePriceEngine");
    bind_engine_price<TurnbullWakemanArithmeticAveragePriceEngine, ArithmeticAveragePriceOption>(arithmetic);

    auto finite_accumulator = bind_finite_difference_engine<FiniteDifferenceAccumulatorEngine>(
        module, "FiniteDifferenceAccumulatorEngine");
    bind_engine_price<FiniteDifferenceAccumulatorEngine, Accumulator>(finite_accumulator);
    auto monte_carlo_accumulator = bind_structured_monte_carlo_engine<MonteCarloAccumulatorEngine>(
        module, "MonteCarloAccumulatorEngine");
    bind_engine_price<MonteCarloAccumulatorEngine, Accumulator>(monte_carlo_accumulator);

    auto finite_snowball = bind_finite_difference_engine<FiniteDifferenceSnowballEngine>(
        module, "FiniteDifferenceSnowballEngine");
    bind_engine_price<FiniteDifferenceSnowballEngine, SnowballOption>(finite_snowball);
    auto finite_binary = bind_finite_difference_engine<FiniteDifferenceBinarySnowballEngine>(
        module, "FiniteDifferenceBinarySnowballEngine");
    bind_engine_price<FiniteDifferenceBinarySnowballEngine, BinarySnowballOption>(finite_binary);
    auto finite_ternary = bind_finite_difference_engine<FiniteDifferenceTernarySnowballEngine>(
        module, "FiniteDifferenceTernarySnowballEngine");
    bind_engine_price<FiniteDifferenceTernarySnowballEngine, TernarySnowballOption>(finite_ternary);
    auto finite_phoenix = bind_finite_difference_engine<FiniteDifferencePhoenixEngine>(
        module, "FiniteDifferencePhoenixEngine");
    bind_engine_price<FiniteDifferencePhoenixEngine, PhoenixOption>(finite_phoenix);

    auto monte_carlo_snowball = bind_structured_monte_carlo_engine<MonteCarloSnowballEngine>(
        module, "MonteCarloSnowballEngine");
    bind_engine_price<MonteCarloSnowballEngine, SnowballOption>(monte_carlo_snowball);
    auto monte_carlo_binary = bind_structured_monte_carlo_engine<MonteCarloBinarySnowballEngine>(
        module, "MonteCarloBinarySnowballEngine");
    bind_engine_price<MonteCarloBinarySnowballEngine, BinarySnowballOption>(monte_carlo_binary);
    auto monte_carlo_ternary = bind_structured_monte_carlo_engine<MonteCarloTernarySnowballEngine>(
        module, "MonteCarloTernarySnowballEngine");
    bind_engine_price<MonteCarloTernarySnowballEngine, TernarySnowballOption>(monte_carlo_ternary);
    auto monte_carlo_phoenix = bind_structured_monte_carlo_engine<MonteCarloPhoenixEngine>(
        module, "MonteCarloPhoenixEngine");
    bind_engine_price<MonteCarloPhoenixEngine, PhoenixOption>(monte_carlo_phoenix);
}

void bind_analytics(nb::module_& module)
{
    bind_engine_analytics<AnalyticVanillaEngine, EuropeanOption>(module);
    bind_engine_analytics<QuadratureVanillaEngine, EuropeanOption>(module);
    bind_engine_analytics<CoxRossRubinsteinVanillaEngine, EuropeanOption, AmericanOption>(module);
    bind_engine_analytics<BjerksundStenslandVanillaEngine, AmericanOption>(module);
    bind_engine_analytics<FiniteDifferenceVanillaEngine, EuropeanOption, AmericanOption>(module);
    bind_engine_analytics<MonteCarloVanillaEngine, EuropeanOption, AmericanOption>(module);
    bind_engine_analytics<AnalyticDigitalEngine, CashOrNothingOption,
                          AssetOrNothingOption>(module);
    bind_engine_analytics<QuadratureDigitalEngine, CashOrNothingOption,
                          AssetOrNothingOption>(module);
    bind_engine_analytics<FiniteDifferenceDigitalEngine, CashOrNothingOption,
                          AssetOrNothingOption>(module);
    bind_engine_analytics<AnalyticBarrierEngine, BarrierOption>(module);
    bind_engine_analytics<FiniteDifferenceBarrierEngine, BarrierOption>(module);
    bind_engine_analytics<AnalyticBinaryBarrierEngine, BinaryBarrierOption, TouchOption>(module);
    bind_engine_analytics<AnalyticGeometricAveragePriceEngine, GeometricAveragePriceOption>(module);
    bind_engine_analytics<TurnbullWakemanArithmeticAveragePriceEngine, ArithmeticAveragePriceOption>(module);
    bind_engine_analytics<FiniteDifferenceAccumulatorEngine, Accumulator>(module);
    bind_engine_analytics<MonteCarloAccumulatorEngine, Accumulator>(module);
    bind_engine_analytics<FiniteDifferenceSnowballEngine, SnowballOption>(module);
    bind_engine_analytics<MonteCarloSnowballEngine, SnowballOption>(module);
    bind_engine_analytics<FiniteDifferenceBinarySnowballEngine, BinarySnowballOption>(module);
    bind_engine_analytics<MonteCarloBinarySnowballEngine, BinarySnowballOption>(module);
    bind_engine_analytics<FiniteDifferenceTernarySnowballEngine, TernarySnowballOption>(module);
    bind_engine_analytics<MonteCarloTernarySnowballEngine, TernarySnowballOption>(module);
    bind_engine_analytics<FiniteDifferencePhoenixEngine, PhoenixOption>(module);
    bind_engine_analytics<MonteCarloPhoenixEngine, PhoenixOption>(module);
    bind_snowball_implied_coupon_pair<FiniteDifferenceSnowballEngine, SnowballOption>(module);
    bind_snowball_implied_coupon_pair<MonteCarloSnowballEngine, SnowballOption>(module);
    bind_snowball_implied_coupon_pair<FiniteDifferenceBinarySnowballEngine, BinarySnowballOption>(module);
    bind_snowball_implied_coupon_pair<MonteCarloBinarySnowballEngine, BinarySnowballOption>(module);
    bind_snowball_implied_coupon_pair<FiniteDifferenceTernarySnowballEngine, TernarySnowballOption>(module);
    bind_snowball_implied_coupon_pair<MonteCarloTernarySnowballEngine, TernarySnowballOption>(module);
    bind_implied_coupon_pair<FiniteDifferencePhoenixEngine, PhoenixOption>(module);
    bind_implied_coupon_pair<MonteCarloPhoenixEngine, PhoenixOption>(module);
}

} // namespace kiyosi::python_binding
