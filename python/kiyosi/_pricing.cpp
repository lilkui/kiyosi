#include "_binding.hpp"
#include <algorithm>

using namespace nb::literals;

namespace kiyosi::python_binding {

namespace {

static_assert(EnumNames<Greek>::values.size() == greek_count);

NumericalShiftSettings numerical_settings(
    double spot_shift, nb::handle volatility_shift, nb::handle rate_shift,
    nb::handle time_shift_days);

GreeksRequest requested_greeks(nb::handle value)
{
    if (nb::isinstance<nb::str>(value))
        return GreeksRequest{string_enum<Greek>(value, "greeks")};
    std::array<Greek, greek_count> selected{};
    std::size_t count = 0;
    nb::iterator iterator;
    try {
        iterator = nb::iter(value);
    } catch (const nb::python_error& error) {
        if (!error.matches(PyExc_TypeError)) throw;
        type_error("greeks", "a Greek name or an iterable of Greek names");
    }
    for (; iterator != nb::iterator::sentinel(); ++iterator) {
        const nb::handle greek = *iterator;
        if (!nb::isinstance<nb::str>(greek))
            type_error("greeks", "a Greek name or an iterable of Greek names");
        const auto choice = string_enum<Greek>(greek, "greeks");
        const auto unique = std::span{selected}.first(count);
        if (std::ranges::find(unique, choice) == unique.end())
            selected[count++] = choice;
    }
    return GreeksRequest{std::span{selected}.first(count)};
}

template <typename Engine, typename Instrument>
void bind_analytics_pair(nb::module_& module);

template <typename Engine, typename Instrument>
struct SpotShiftArgument {
    using Type = PythonReal;
    static nb::object default_value() { return nb::float_(NumericalShiftSettings{}.spot_shift); }
    static double convert(Type value, const PricingContext&) { return real_number(value, "spot_shift"); }
};

template <>
struct SpotShiftArgument<MonteCarloVanillaEngine, AmericanOption> {
    using Type = nb::typed<nb::handle, std::optional<double>>;
    static nb::object default_value() { return nb::none(); }
    static double convert(Type value, const PricingContext& context)
    {
        return value.is_none() ? MonteCarloVanillaEngine::default_american_spot_shift(context)
                               : real_number(value, "spot_shift");
    }
};

template <typename Engine, typename Instrument>
double price_for_python(const Engine& engine, const Instrument& instrument, const PricingContext& context)
{
    // Releasing the GIL costs more than these short scalar formulas.
    if constexpr (std::same_as<Engine, AnalyticVanillaEngine> ||
                  std::same_as<Engine, AnalyticDigitalEngine> ||
                  std::same_as<Engine, AnalyticGeometricAveragePriceEngine> ||
                  std::same_as<Engine, TurnbullWakemanArithmeticAveragePriceEngine>) {
        return unwrap(engine.price(instrument, context));
    } else if constexpr (std::same_as<Engine, AnalyticBarrierEngine> ||
                         std::same_as<Engine, AnalyticBinaryBarrierEngine>) {
        // Scheduled monitoring validates calendars across the full observation schedule.
        if (instrument.observation_mode() != ObservationMode::scheduled)
            return unwrap(engine.price(instrument, context));
    }
    nb::gil_scoped_release release;
    return unwrap(engine.price(instrument, context));
}

template <typename Engine, typename Instrument>
PricingResult greeks_for_python(const Engine& engine, const Instrument& instrument, const PricingContext& context,
                                GreeksRequest request, const NumericalShiftSettings& settings)
{
    if constexpr (std::same_as<Engine, AnalyticVanillaEngine> ||
                  std::same_as<Engine, AnalyticDigitalEngine>) {
        return unwrap(engine.price_with_greeks(instrument, context, request, settings));
    } else if constexpr (std::same_as<Engine, AnalyticGeometricAveragePriceEngine> ||
                         std::same_as<Engine, TurnbullWakemanArithmeticAveragePriceEngine>) {
        // Three short prices cost less than releasing and reacquiring the GIL.
        if (request.has(Greek::delta) && std::ranges::none_of(
                                             std::array{Greek::gamma, Greek::speed, Greek::theta, Greek::charm, Greek::color,
                                                        Greek::vega, Greek::vanna, Greek::zomma, Greek::rho},
                                             [&](Greek greek) { return request.has(greek); }))
            return unwrap(engine.price_with_greeks(instrument, context, request, settings));
    }
    nb::gil_scoped_release release;
    return unwrap(engine.price_with_greeks(instrument, context, request, settings));
}

template <typename Engine, typename Instrument>
void bind_engine_price(nb::module_& module, nb::class_<Engine>& binding)
{
    bind_analytics_pair<Engine, Instrument>(module);
    using SpotShift = SpotShiftArgument<Engine, Instrument>;
    binding.def(
        "price",
        &price_for_python<Engine, Instrument>,
        "instrument"_a, "context"_a,
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
    the requested valuation are invalid.

Notes
-----
Operational exceptions and cancellation follow the package-level contract in
:mod:`kiyosi`: allocation failures raise MemoryError, and Python signal handling
is deferred until the synchronous core computation returns.)doc");
    binding.def(
        "price_with_greeks",
        [](const Engine& engine, const Instrument& instrument, const PricingContext& context,
           PythonGreekRequest greeks, nb::typed<nb::handle, bool> all_greeks, typename SpotShift::Type spot_shift, PythonReal volatility_shift,
           PythonReal rate_shift, PythonInteger time_shift_days) {
            if (!PyBool_Check(all_greeks.ptr())) type_error("all_greeks", "a bool");
            const bool all = all_greeks.ptr() == Py_True;
            if (all && !greeks.is_none()) type_error("greeks", "omitted when all_greeks is True");
            if (!all && greeks.is_none()) type_error("greeks", "provided unless all_greeks is True");
            const GreeksRequest request = all ? GreeksRequest{true} : requested_greeks(greeks);
            const auto settings = numerical_settings(
                SpotShift::convert(spot_shift, context), volatility_shift, rate_shift, time_shift_days);
            return greeks_for_python(engine, instrument, context, request, settings);
        },
        "instrument"_a, "context"_a, "greeks"_a = nb::none(), nb::kw_only(),
        "all_greeks"_a = false,
        "spot_shift"_a.none() = SpotShift::default_value(),
        "volatility_shift"_a = NumericalShiftSettings{}.volatility_shift,
        "rate_shift"_a = NumericalShiftSettings{}.rate_shift,
        "time_shift_days"_a = NumericalShiftSettings{}.time_shift_days,
        R"doc(Compute price and the explicitly selected Greeks.

Pass one lowercase Greek name or an iterable of them, or set ``all_greeks=True`` to
request all ten. These forms are mutually exclusive. Native values are reused;
engines supporting numerical completion fill missing feasible measures using numerical
price differences. Digital and barrier Monte Carlo and CRR engines support only delta and gamma;
digital MC/CRR and barrier CRR validate shift settings without using them. Barrier MC
uses spot shifts for the deterministic continuous at-hit rebate sensitivities.
Unrequested, unsupported or
undefined measures are None, never zero sentinels. At expiry or a monitored
barrier hit-state boundary, only price is available. Feasible bumped valuation
failures fail the entire operation.
At current autocallable-product event thresholds, undefined spot and time
sensitivities remain None while feasible rate and volatility sensitivities survive.

Shift keyword arguments are absolute and use core-owned defaults. Spot shifts
use asset-price units, volatility and rate shifts use decimal units, and time
shifts use calendar days. Collapsed or asymmetric stencils leave affected
numerical Greeks unavailable. Monte Carlo valuations share one seed per request
without changing the engine. Concurrent calls are safe.
For American Monte Carlo, omitted or None spot_shift uses 8% of spot to reduce
exercise-decision sampling noise. Explicit shifts remain absolute. Sampling error
and bump-size bias remain; check convergence for the valuation.

Returns
-------
PricingResult
    Price and requested available Greeks, with Greek units.

Raises
------
TypeError
    If a Greek representation, engine/instrument pairing, or shift representation is incompatible.
OverflowError
    If time_shift_days is outside the C++ int range.
KiyosiError
    If the Greek request or settings are invalid, or a required valuation fails.

Notes
-----
Operational exceptions and cancellation follow the package-level contract in
:mod:`kiyosi`: allocation failures raise MemoryError, and Python signal handling
is deferred until the synchronous core computation returns.)doc");
}

template <typename Engine>
nb::class_<Engine> bind_stateless_engine(nb::module_& module, const char* name)
{
    std::string description = R"doc(Stateless pricing engine.

The engine has no configuration and may be reused across supported instruments
and contexts. Concurrent calls are safe.)doc";
    if constexpr (std::same_as<Engine, AnalyticBarrierEngine> || std::same_as<Engine, AnalyticBinaryBarrierEngine>)
        description += R"doc(

Scheduled monitoring is approximated with a BGK barrier shift. Untouched contracts
with future observations require a final observation at expiry; shorter monitoring
windows raise KiyosiError with category UNSUPPORTED_OPERATION. Spot must lie strictly
on the unbreached side of the shifted barrier; otherwise the approximation raises
the same error. Resolved settlements remain supported.)doc";
    nb::class_<Engine> binding{module, name, description.c_str()};
    binding.def(nb::init<>(), R"doc(Create a stateless pricing engine.

Returns
-------
pricing engine
    Reusable engine with no mutable configuration.)doc");
    bind_repr(binding, name, {});
    return binding;
}

template <typename Engine>
nb::class_<Engine> bind_finite_difference_engine(nb::module_& module, const char* name,
                                                 Result<Engine> (*factory)(FiniteDifferenceSettings))
{
    nb::class_<Engine> binding{
        module, name,
        R"doc(Finite-difference pricing engine with immutable configuration.

Configuration is validated at construction. Contract, market, and backend availability
are checked when pricing.
Asset steps must be at least 3 and time steps at least 1, within the C++ int range.
Pricing is subject to available memory and runtime; allocation failures raise
MemoryError. Explicit Euler also requires a stable grid for the chosen market.)doc"};
    binding
        .def(nb::new_([factory](PythonInteger asset_step_count, PythonInteger time_step_count,
                                PythonChoice<FiniteDifferenceScheme> scheme, PythonReal asset_upper_boundary) {
                 std::optional<double> boundary;
                 if (!asset_upper_boundary.is_none())
                     boundary = real_number(asset_upper_boundary, "asset_upper_boundary");
                 return unwrap(factory(FiniteDifferenceSettings{
                     integer(asset_step_count, "asset_step_count"), integer(time_step_count, "time_step_count"),
                     scheme, boundary}));
             }),
             nb::kw_only(), "asset_step_count"_a = Engine{}.settings().asset_step_count,
             "time_step_count"_a = Engine{}.settings().time_step_count,
             "scheme"_a = Engine{}.settings().scheme,
             "asset_upper_boundary"_a.none() = Engine{}.settings().asset_upper_boundary,
             R"doc(Create an engine with validated finite-difference settings.

Settings are validated at construction.
The engine-specific ranges are listed in the engine class documentation.

Parameters
----------
asset_step_count : int, optional
    Number of spatial grid steps. Uses the core default when omitted.
time_step_count : int, optional
    Number of time grid steps. Uses the core default when omitted.
scheme : {'explicit_euler', 'implicit_euler', 'crank_nicolson'}, optional
    Time-stepping scheme. Uses the core default when omitted.
asset_upper_boundary : float or None, optional
    Explicit upper asset-grid boundary, or ``None`` for the core default.

Raises
------
TypeError
    If a setting has an incompatible representation.
OverflowError
    If an integer setting is outside the C++ ``int`` range.
KiyosiError
    INVALID_PARAMETER if the core rejects the configuration.)doc")
        .def_prop_ro("asset_step_count", [](const Engine& engine) { return engine.settings().asset_step_count; }, "Number of spatial grid steps.")
        .def_prop_ro("time_step_count", [](const Engine& engine) { return engine.settings().time_step_count; }, "Number of time grid steps.")
        .def_prop_ro("scheme", [](const Engine& engine) { return engine.settings().scheme; }, "Finite-difference time-stepping scheme.")
        .def_prop_ro("asset_upper_boundary", [](const Engine& engine) { return engine.settings().asset_upper_boundary; }, "Explicit upper asset-grid boundary, or None for the core default.");
    bind_repr(binding, name,
              {"asset_step_count", "time_step_count", "scheme", "asset_upper_boundary"});
    return binding;
}

template <typename Engine>
nb::class_<Engine> bind_quadrature_engine(nb::module_& module, const char* name,
                                          Result<Engine> (*factory)(QuadratureSettings))
{
    nb::class_<Engine> binding{module, name,
                               "Event-date BSM integration with immutable settings. Check space and domain convergence separately; concurrent calls are safe. Calls are synchronous without cancellation; allocation failures raise MemoryError."};
    binding
        .def(nb::new_([factory](PythonInteger asset_step_count, PythonReal integration_standard_deviations) {
                 return unwrap(factory({integer(asset_step_count, "asset_step_count"),
                                        real_number(integration_standard_deviations, "integration_standard_deviations")}));
             }),
             nb::kw_only(), "asset_step_count"_a = QuadratureSettings{}.asset_step_count,
             "integration_standard_deviations"_a = QuadratureSettings{}.integration_standard_deviations,
             "Create an engine with at least four spatial steps and a finite positive integration extent. Representation errors raise TypeError/OverflowError; core configuration rejections raise KiyosiError.")
        .def_prop_ro("asset_step_count", [](const Engine& engine) { return engine.settings().asset_step_count; })
        .def_prop_ro("integration_standard_deviations", [](const Engine& engine) { return engine.settings().integration_standard_deviations; });
    bind_repr(binding, name, {"asset_step_count", "integration_standard_deviations"});
    return binding;
}

template <typename Engine>
nb::class_<Engine> bind_trading_day_monte_carlo_engine(nb::module_& module, const char* name,
                                                       Result<Engine> (*factory)(TradingDayMonteCarloSettings))
{
    nb::class_<Engine> binding{
        module, name,
        R"doc(Trading-day Monte Carlo engine with immutable configuration.

Configuration is validated at construction. Contract, market, and backend availability
are checked when pricing.
Accumulator and autocallable engines accept 1..10,000,000 paths.
Sobol sampling requires a power-of-two path count and the CPU backend.)doc"};
    binding
        .def(nb::new_([factory](PythonInteger path_count, PythonInteger seed, PythonChoice<MonteCarloBackend> backend, PythonInteger thread_count, PythonChoice<MonteCarloSampling> sampling, bool antithetic) {
                 return unwrap(factory(TradingDayMonteCarloSettings{
                     integer(path_count, "path_count"), optional_seed(seed),
                     backend, integer(thread_count, "thread_count"), sampling, antithetic}));
             }),
             nb::kw_only(), "path_count"_a = TradingDayMonteCarloSettings{}.path_count,
             "seed"_a.none() = TradingDayMonteCarloSettings{}.seed,
             "backend"_a = TradingDayMonteCarloSettings{}.backend,
             "thread_count"_a = TradingDayMonteCarloSettings{}.thread_count,
             "sampling"_a = TradingDayMonteCarloSettings{}.sampling,
             "antithetic"_a.noconvert() = TradingDayMonteCarloSettings{}.antithetic,
             R"doc(Create an engine with validated Monte Carlo settings.

Settings are validated at construction.

Parameters
----------
path_count : int, optional
    Number of simulated paths. Uses the core default when omitted.
seed : int or None, optional
    Non-negative random seed. Uses the core default when omitted.
    Early knock-out does not shift later paths' samples.
backend : {'cpu', 'cuda'}, optional
    CPU or CUDA execution backend.
thread_count : int, optional
    Per-call CPU concurrency cap: 0 automatic, 1 serial. CUDA requires 0.
    Seeded CPU results are identical across thread counts in the same build.
sampling : {'pseudo_random', 'sobol'}, optional
    Sampling method. Sobol uses one seed-reproducible LMS-scrambled and digitally
    shifted point per base path, with chronological normal increments and up to 21,201
    dimensions. Set a power-of-two path_count explicitly; the core default is not
    a power of two. Sobol supports only the CPU backend.

antithetic : bool, optional
    Pair normal increments Z and -Z; defaults to False. path_count is the exact
    total: an odd count leaves one path unpaired. Sobol uses half as many points
    when enabled, except path_count=1 uses one unpaired point.

Raises
------
TypeError
    If a setting has an incompatible representation.
OverflowError
    If an integer is outside its C++ representation range.
KiyosiError
    INVALID_PARAMETER if the core rejects the configuration.
    UNSUPPORTED_OPERATION for Sobol with CUDA.)doc")
        .def_prop_ro("path_count", [](const Engine& engine) { return engine.settings().path_count; }, "Number of simulated paths.")
        .def_prop_ro("seed", [](const Engine& engine) { return engine.settings().seed; }, "Non-negative random seed.")
        .def_prop_ro("backend", [](const Engine& engine) { return engine.settings().backend; }, "CPU or CUDA execution backend.")
        .def_prop_ro("thread_count", [](const Engine& engine) { return engine.settings().thread_count; }, "Per-call CPU concurrency cap; 0 automatic, 1 serial.")
        .def_prop_ro("antithetic", [](const Engine& engine) { return engine.settings().antithetic; }, "Whether to use antithetic variates.")
        .def_prop_ro("sampling", [](const Engine& engine) { return engine.settings().sampling; }, "Pseudorandom or scrambled Sobol sampling.");
    bind_repr(binding, name,
              {"path_count", "seed", "backend", "thread_count", "sampling", "antithetic"});
    return binding;
}

NumericalShiftSettings numerical_settings(
    double spot_shift, nb::handle volatility_shift, nb::handle rate_shift,
    nb::handle time_shift_days)
{
    return {spot_shift,
            real_number(volatility_shift, "volatility_shift"),
            real_number(rate_shift, "rate_shift"), integer(time_shift_days, "time_shift_days")};
}

template <typename Settings>
Settings implied_settings(
    nb::handle lower_bound, nb::handle upper_bound, nb::handle price_tolerance, nb::handle parameter_tolerance,
    nb::handle max_iterations)
{
    return {real_number(lower_bound, "lower_bound"),
            real_number(upper_bound, "upper_bound"), real_number(price_tolerance, "price_tolerance"),
            real_number(parameter_tolerance, "parameter_tolerance"),
            integer(max_iterations, "max_iterations")};
}

template <typename Engine, typename Instrument>
void bind_analytics_pair(nb::module_& module)
{
    module.def(
        "implied_volatility",
        [](const Engine& engine, const Instrument& instrument, const PricingContext& context,
           PythonReal observed_price, PythonReal lower_bound, PythonReal upper_bound,
           PythonReal price_tolerance, PythonReal parameter_tolerance, PythonInteger max_iterations) {
            const double observed = real_number(observed_price, "observed_price");
            const auto settings = implied_settings<ImpliedVolatilitySettings>(
                lower_bound, upper_bound, price_tolerance, parameter_tolerance, max_iterations);
            nb::gil_scoped_release release;
            return unwrap(kiyosi::implied_volatility(
                engine, instrument, context, observed, settings));
        },
        "engine"_a, "instrument"_a, "context"_a, "observed_price"_a, nb::kw_only(),
        "lower_bound"_a = ImpliedVolatilitySettings{}.lower_bound,
        "upper_bound"_a = ImpliedVolatilitySettings{}.upper_bound,
        "price_tolerance"_a = ImpliedVolatilitySettings{}.price_tolerance,
        "parameter_tolerance"_a = ImpliedVolatilitySettings{}.parameter_tolerance,
        "max_iterations"_a = ImpliedVolatilitySettings{}.max_iterations,
        R"doc(Solve for the volatility that matches an observed price.

Uses safeguarded Brent interpolation, with bisection when needed to retain the bracket.

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
    CRR searches intersect this interval with the tree's admissible volatility range.
price_tolerance : float, optional
    Absolute price-error tolerance; Monte Carlo sampling error is not bounded.
parameter_tolerance : float, optional
    Absolute search-interval tolerance in volatility or coupon-rate units.
    The solver stops when either tolerance is met.
max_iterations : int, optional
    Maximum solver iterations.

Returns
-------
float
    Implied volatility as a decimal rate.

An unseeded Monte Carlo engine uses one random seed throughout this solve.
Known volatility-independent payments raise ``KiyosiError`` with category
``UNSUPPORTED_OPERATION``.

Raises
------
TypeError
    If the engine/instrument combination or an argument is incompatible.
KiyosiError
    If inputs are invalid, the target is not bracketed, or the solver fails.)doc");
}

constexpr const char* implied_coupon_doc = R"doc(Solve for a coupon rate that matches an observed price.

Parameters
----------
engine : pricing engine
    Engine used for trial valuations.
instrument : SnowballOption, BinarySnowballOption, TernarySnowballOption, or PhoenixOption
    Instrument supported by the engine.
context : PricingContext
    Market state used by the solver.
observed_price : float
    Target instrument price.
quote_convention : {'shift_maturity_coupon', 'preserve_maturity_coupon'}
    Required for snowball instruments: whether the maturity coupon shifts with
    quoted knock-out coupons. Omit this argument for Phoenix instruments.
lower_bound, upper_bound : float, optional
    Finite coupon-rate search interval, which may include negative rates. Omitted values use core defaults.
price_tolerance : float, optional
    Absolute price-error tolerance; Monte Carlo sampling error is not bounded.
parameter_tolerance : float, optional
    Absolute search-interval tolerance in volatility or coupon-rate units.
    The solver stops when either tolerance is met.
max_iterations : int, optional
    Maximum solver iterations.

Returns
-------
float
    Implied annualized coupon rate.

An unseeded Monte Carlo engine uses one random seed throughout this solve.
Monte Carlo and finite-difference engines first try interpolation and verify its
price residual. If the price tolerance is not met, the solver uses bisection.

Raises
------
TypeError
    If the engine/instrument combination or an argument is incompatible.
KiyosiError
    If inputs are invalid, the target is not bracketed, or the solver fails.)doc";

template <typename Engine, typename Instrument>
void bind_implied_coupon_pair(nb::module_& module)
{
    module.def(
        "implied_coupon",
        [](const Engine& engine, const Instrument& instrument, const PricingContext& context,
           PythonReal observed_price, PythonReal lower_bound, PythonReal upper_bound,
           PythonReal price_tolerance, PythonReal parameter_tolerance, PythonInteger max_iterations) {
            const double observed = real_number(observed_price, "observed_price");
            const auto settings = implied_settings<ImpliedCouponSettings>(
                lower_bound, upper_bound, price_tolerance, parameter_tolerance, max_iterations);
            nb::gil_scoped_release release;
            return unwrap(kiyosi::implied_coupon(
                engine, instrument, context, observed, settings));
        },
        "engine"_a, "instrument"_a, "context"_a, "observed_price"_a, nb::kw_only(),
        "lower_bound"_a = ImpliedCouponSettings{}.lower_bound,
        "upper_bound"_a = ImpliedCouponSettings{}.upper_bound,
        "price_tolerance"_a = ImpliedCouponSettings{}.price_tolerance,
        "parameter_tolerance"_a = ImpliedCouponSettings{}.parameter_tolerance,
        "max_iterations"_a = ImpliedCouponSettings{}.max_iterations,
        implied_coupon_doc);
}

template <typename Engine, typename Instrument>
void bind_snowball_implied_coupon_pair(nb::module_& module)
{
    module.def(
        "implied_coupon",
        [](const Engine& engine, const Instrument& instrument, const PricingContext& context,
           PythonReal observed_price, PythonChoice<CouponQuoteConvention> quote_convention,
           PythonReal lower_bound, PythonReal upper_bound, PythonReal price_tolerance, PythonReal parameter_tolerance,
           PythonInteger max_iterations) {
            const double observed = real_number(observed_price, "observed_price");
            const CouponQuoteConvention convention = quote_convention;
            const auto settings = implied_settings<ImpliedCouponSettings>(
                lower_bound, upper_bound, price_tolerance, parameter_tolerance, max_iterations);
            nb::gil_scoped_release release;
            return unwrap(kiyosi::implied_coupon(
                engine, instrument, context, observed, convention, settings));
        },
        "engine"_a, "instrument"_a, "context"_a, "observed_price"_a, nb::kw_only(),
        "quote_convention"_a,
        "lower_bound"_a = ImpliedCouponSettings{}.lower_bound,
        "upper_bound"_a = ImpliedCouponSettings{}.upper_bound,
        "price_tolerance"_a = ImpliedCouponSettings{}.price_tolerance,
        "parameter_tolerance"_a = ImpliedCouponSettings{}.parameter_tolerance,
        "max_iterations"_a = ImpliedCouponSettings{}.max_iterations,
        implied_coupon_doc);
}

} // namespace

void bind_results(nb::module_& module)
{
    auto pricing_result = nb::class_<PricingResult>(
                              module, "PricingResult",
                              R"doc(Read-only price and Greek result.

Price uses instrument value units. Delta, gamma, and speed are per one spot unit,
squared spot unit, and cubed spot unit. Vega, vanna, and zomma are price, delta,
and gamma changes per one volatility percentage point (an absolute change of
0.01). Rho is per one interest-rate percentage point. Theta, charm, and color
are price, delta, and gamma changes per calendar day as valuation time moves
forward. Undefined or unsupported Greeks are None, never a zero sentinel.

Attributes
----------
price : float
    Instrument value.
delta, gamma, speed : float or None
    First three spot derivatives.
theta, charm, color : float or None
    Daily changes in price, delta, and gamma.
vega, vanna, zomma : float or None
    Volatility-point changes in price, delta, and gamma.
rho : float or None
    Price change per interest-rate percentage point.)doc")
                              .def("require", [](const PricingResult& result, PythonChoice<Greek> measure) { return unwrap(result.require(measure)); }, "measure"_a, R"doc(Return a required Greek.

Parameters
----------
measure : {'delta', 'gamma', 'speed', 'theta', 'charm', 'color', 'vega', 'vanna', 'zomma', 'rho'}
    Greek to retrieve.

Returns
-------
float
    Available Greek value.

Raises
------
KiyosiError
    If the requested Greek is unavailable.)doc")
                              .def_prop_ro("price", &PricingResult::price, "Instrument value.")
                              .def_prop_ro("delta", [](const PricingResult& result) { return unwrap(result.get(Greek::delta)); }, "First spot derivative, or None when unavailable.")
                              .def_prop_ro("gamma", [](const PricingResult& result) { return unwrap(result.get(Greek::gamma)); }, "Second spot derivative, or None when unavailable.")
                              .def_prop_ro("speed", [](const PricingResult& result) { return unwrap(result.get(Greek::speed)); }, "Third spot derivative, or None when unavailable.")
                              .def_prop_ro("theta", [](const PricingResult& result) { return unwrap(result.get(Greek::theta)); }, "Daily price decay, or None when unavailable.")
                              .def_prop_ro("charm", [](const PricingResult& result) { return unwrap(result.get(Greek::charm)); }, "Daily change in delta, or None when unavailable.")
                              .def_prop_ro("color", [](const PricingResult& result) { return unwrap(result.get(Greek::color)); }, "Daily change in gamma, or None when unavailable.")
                              .def_prop_ro("vega", [](const PricingResult& result) { return unwrap(result.get(Greek::vega)); }, "Price change per volatility percentage point, or None.")
                              .def_prop_ro("vanna", [](const PricingResult& result) { return unwrap(result.get(Greek::vanna)); }, "Delta change per volatility percentage point, or None.")
                              .def_prop_ro("zomma", [](const PricingResult& result) { return unwrap(result.get(Greek::zomma)); }, "Gamma change per volatility percentage point, or None.")
                              .def_prop_ro("rho", [](const PricingResult& result) { return unwrap(result.get(Greek::rho)); }, "Price change per interest-rate percentage point, or None.");
    bind_repr(pricing_result, "PricingResult",
              {"price", "delta", "gamma", "speed", "theta", "charm", "color", "vega", "vanna", "zomma", "rho"});
}

void bind_engines(nb::module_& module)
{
    auto analytic_vanilla = bind_stateless_engine<AnalyticVanillaEngine>(
        module, "AnalyticVanillaEngine");
    bind_engine_price<AnalyticVanillaEngine, EuropeanOption>(module, analytic_vanilla);
    auto integral_vanilla = bind_stateless_engine<QuadratureVanillaEngine>(
        module, "QuadratureVanillaEngine");
    bind_engine_price<QuadratureVanillaEngine, EuropeanOption>(module, integral_vanilla);
    auto crr = nb::class_<CoxRossRubinsteinVanillaEngine>(
                   module, "CoxRossRubinsteinVanillaEngine",
                   R"doc(Cox-Ross-Rubinstein binomial vanilla-option engine.

Configuration is immutable and validated at construction. Market-dependent tree
constraints are checked when pricing.)doc")
                   .def(nb::new_([](PythonInteger step_count) {
                            return unwrap(make_cox_ross_rubinstein_vanilla_engine({integer(step_count, "step_count")}));
                        }),
                        "step_count"_a = BinomialSettings{}.step_count,
                        R"doc(Create an engine with validated Cox-Ross-Rubinstein settings.

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
KiyosiError
    INVALID_PARAMETER if the step count is outside 1..1,000,000.

Notes
-----
The step count is validated at construction; accepted range is 1..1,000,000.)doc")
                   .def_prop_ro("step_count", [](const CoxRossRubinsteinVanillaEngine& engine) { return engine.settings().step_count; }, "Number of binomial time steps.");
    bind_repr(crr, "CoxRossRubinsteinVanillaEngine", {"step_count"});
    bind_engine_price<CoxRossRubinsteinVanillaEngine, EuropeanOption>(module, crr);
    bind_engine_price<CoxRossRubinsteinVanillaEngine, AmericanOption>(module, crr);
    auto bjerksund = bind_stateless_engine<BjerksundStenslandVanillaEngine>(
        module, "BjerksundStenslandVanillaEngine");
    bind_engine_price<BjerksundStenslandVanillaEngine, AmericanOption>(module, bjerksund);
    auto finite_vanilla = bind_finite_difference_engine<FiniteDifferenceVanillaEngine>(
        module, "FiniteDifferenceVanillaEngine", &make_finite_difference_vanilla_engine);
    bind_engine_price<FiniteDifferenceVanillaEngine, EuropeanOption>(module, finite_vanilla);
    bind_engine_price<FiniteDifferenceVanillaEngine, AmericanOption>(module, finite_vanilla);
    auto monte_carlo_vanilla = nb::class_<MonteCarloVanillaEngine>(
                                   module, "MonteCarloVanillaEngine",
                                   R"doc(Monte Carlo vanilla-option engine with immutable configuration.

Configuration is validated at construction. Contract, market, and backend availability
are checked when pricing.
Accepts 1..10,000,000 paths and 2..10,000 steps; American pricing before
expiry requires at least 3 steps.
European prices sample the exact terminal distribution; valid step counts do not affect them.
American calls with nonpositive dividends and nonnegative rates use that distribution
with an immediate-exercise lower bound. Rate stencils crossing zero retain exercise-grid
simulations on both sides.)doc")
                                   .def(nb::new_([](PythonInteger path_count, PythonInteger step_count,
                                                    PythonInteger seed, PythonChoice<MonteCarloBackend> backend, PythonInteger thread_count, PythonChoice<MonteCarloSampling> sampling, bool antithetic) {
                                            return unwrap(make_monte_carlo_vanilla_engine(MonteCarloSettings{
                                                integer(path_count, "path_count"), integer(step_count, "step_count"),
                                                optional_seed(seed), backend, integer(thread_count, "thread_count"), sampling, antithetic}));
                                        }),
                                        nb::kw_only(), "path_count"_a = MonteCarloSettings{}.path_count,
                                        "step_count"_a = MonteCarloSettings{}.step_count,
                                        "seed"_a.none() = MonteCarloSettings{}.seed,
                                        "backend"_a = MonteCarloSettings{}.backend,
                                        "thread_count"_a = MonteCarloSettings{}.thread_count,
                                        "sampling"_a = MonteCarloSettings{}.sampling,
                                        "antithetic"_a.noconvert() = MonteCarloSettings{}.antithetic,
                                        R"doc(Create an engine with validated Monte Carlo settings.

Parameters
----------
path_count : int, optional
    Number of simulated paths. Uses the core default when omitted.
step_count : int, optional
    American time grid points; validated but unused for European pricing.
    Uses the core default when omitted.
seed : int or None, optional
    Non-negative random seed, or ``None`` for nondeterministic seeding.
backend : {'cpu', 'cuda'}, optional
    CPU or CUDA execution backend.
thread_count : int, optional
    Per-call CPU concurrency cap: 0 automatic, 1 serial. CUDA requires 0.
    Seeded CPU results are identical across thread counts in the same build.
sampling : {'pseudo_random', 'sobol'}, optional
    Sampling method. Sobol uses one seed-reproducible LMS-scrambled and digitally
    shifted point per base path, with chronological normal
    increments and up to 21,201 dimensions. Set a power-of-two path_count
    explicitly; the core default is not a power of two. Sobol supports only CPU.

antithetic : bool, optional
    Pair normal increments Z and -Z; defaults to False. path_count is the exact
    total: an odd count leaves one path unpaired. Sobol uses half as many points
    when enabled, except path_count=1 uses one unpaired point.

Raises
------
TypeError
    If a setting has an incompatible representation.
OverflowError
    If an integer is outside its C++ representation range.
KiyosiError
    INVALID_PARAMETER if the core rejects the configuration.
    UNSUPPORTED_OPERATION for Sobol with CUDA.

Notes
-----
Settings are validated at construction.)doc")
                                   .def_prop_ro("path_count", [](const MonteCarloVanillaEngine& engine) { return engine.settings().path_count; }, "Number of simulated paths.")
                                   .def_prop_ro("step_count", [](const MonteCarloVanillaEngine& engine) { return engine.settings().step_count; }, "American time grid points; validated but unused for European pricing.")
                                   .def_prop_ro("seed", [](const MonteCarloVanillaEngine& engine) { return engine.settings().seed; }, "Optional non-negative random seed.")
                                   .def_prop_ro("backend", [](const MonteCarloVanillaEngine& engine) { return engine.settings().backend; }, "CPU or CUDA execution backend.")
                                   .def_prop_ro("thread_count", [](const MonteCarloVanillaEngine& engine) { return engine.settings().thread_count; }, "Per-call CPU concurrency cap; 0 automatic, 1 serial.")
                                   .def_prop_ro("antithetic", [](const MonteCarloVanillaEngine& engine) { return engine.settings().antithetic; }, "Whether to use antithetic variates.")
                                   .def_prop_ro("sampling", [](const MonteCarloVanillaEngine& engine) { return engine.settings().sampling; }, "Pseudorandom or scrambled Sobol sampling.");
    bind_repr(monte_carlo_vanilla, "MonteCarloVanillaEngine",
              {"path_count", "step_count", "seed", "backend", "thread_count", "sampling", "antithetic"});
    bind_engine_price<MonteCarloVanillaEngine, EuropeanOption>(module, monte_carlo_vanilla);
    bind_engine_price<MonteCarloVanillaEngine, AmericanOption>(module, monte_carlo_vanilla);

    auto crr_digital = nb::class_<CoxRossRubinsteinDigitalEngine>(
                           module, "CoxRossRubinsteinDigitalEngine",
                           "European cash/asset digital CRR pricing with node delta/gamma. Other Greeks are unavailable. "
                           "Price convergence can oscillate with strike alignment; gamma requires at least two steps.")
                           .def(nb::new_([](PythonInteger step_count) {
                                    return unwrap(make_cox_ross_rubinstein_digital_engine({integer(step_count, "step_count")}));
                                }),
                                "step_count"_a = BinomialSettings{}.step_count,
                                "Create an engine with 1..1,000,000 steps. Invalid representations raise TypeError/OverflowError; invalid counts raise KiyosiError.")
                           .def_prop_ro("step_count", [](const CoxRossRubinsteinDigitalEngine& engine) { return engine.settings().step_count; });
    bind_repr(crr_digital, "CoxRossRubinsteinDigitalEngine", {"step_count"});
    bind_engine_price<CoxRossRubinsteinDigitalEngine, CashOrNothingOption>(module, crr_digital);
    bind_engine_price<CoxRossRubinsteinDigitalEngine, AssetOrNothingOption>(module, crr_digital);

    auto mc_digital = nb::class_<MonteCarloDigitalEngine>(
                          module, "MonteCarloDigitalEngine",
                          "Direct exact-terminal European cash/asset digital Monte Carlo pricing with likelihood-ratio delta/gamma. "
                          "Other Greeks are unavailable. Estimates have sampling error, especially near expiry. "
                          "CPU supports pseudorandom/Sobol; optional CUDA supports pseudorandom. All storage is request-local.")
                          .def(nb::new_([](PythonInteger path_count, PythonInteger step_count, PythonInteger seed,
                                           PythonChoice<MonteCarloBackend> backend, PythonInteger thread_count,
                                           PythonChoice<MonteCarloSampling> sampling, bool antithetic) {
                                   return unwrap(make_monte_carlo_digital_engine(MonteCarloSettings{
                                       integer(path_count, "path_count"), integer(step_count, "step_count"), optional_seed(seed),
                                       backend, integer(thread_count, "thread_count"), sampling, antithetic}));
                               }),
                               nb::kw_only(), "path_count"_a = MonteCarloSettings{}.path_count,
                               "step_count"_a = MonteCarloSettings{}.step_count, "seed"_a.none() = MonteCarloSettings{}.seed,
                               "backend"_a = MonteCarloSettings{}.backend, "thread_count"_a = MonteCarloSettings{}.thread_count,
                               "sampling"_a = MonteCarloSettings{}.sampling, "antithetic"_a.noconvert() = MonteCarloSettings{}.antithetic,
                               R"doc(Create a digital engine using the core MonteCarloSettings defaults.

path_count is the exact number of paths (1..10,000,000). step_count is validated
(2..10,000) but unused. seed is a nonnegative integer or None for system entropy.
backend is 'cpu' or 'cuda'; sampling is 'pseudo_random' or CPU-only 'sobol'.
Sobol requires an explicit power-of-two path_count. thread_count is a per-call CPU
cap (0 automatic, 1 serial); CUDA requires 0. antithetic pairs Z and -Z, leaving
one path unpaired for odd counts. Seeded CPU results reproduce across thread counts
in the same build; CPU and CUDA streams differ.
Invalid representations raise TypeError/OverflowError. Core configuration failures
raise KiyosiError (INVALID_PARAMETER or UNSUPPORTED_OPERATION for CUDA with Sobol).
Backend availability is checked when pricing. Operational failures follow the
package contract.)doc")
                          .def_prop_ro("path_count", [](const MonteCarloDigitalEngine& engine) { return engine.settings().path_count; })
                          .def_prop_ro("step_count", [](const MonteCarloDigitalEngine& engine) { return engine.settings().step_count; })
                          .def_prop_ro("seed", [](const MonteCarloDigitalEngine& engine) { return engine.settings().seed; })
                          .def_prop_ro("backend", [](const MonteCarloDigitalEngine& engine) { return engine.settings().backend; })
                          .def_prop_ro("thread_count", [](const MonteCarloDigitalEngine& engine) { return engine.settings().thread_count; })
                          .def_prop_ro("sampling", [](const MonteCarloDigitalEngine& engine) { return engine.settings().sampling; })
                          .def_prop_ro("antithetic", [](const MonteCarloDigitalEngine& engine) { return engine.settings().antithetic; });
    bind_repr(mc_digital, "MonteCarloDigitalEngine", {"path_count", "step_count", "seed", "backend", "thread_count", "sampling", "antithetic"});
    bind_engine_price<MonteCarloDigitalEngine, CashOrNothingOption>(module, mc_digital);
    bind_engine_price<MonteCarloDigitalEngine, AssetOrNothingOption>(module, mc_digital);

    auto analytic_digital = bind_stateless_engine<AnalyticDigitalEngine>(
        module, "AnalyticDigitalEngine");
    bind_engine_price<AnalyticDigitalEngine, CashOrNothingOption>(module, analytic_digital);
    bind_engine_price<AnalyticDigitalEngine, AssetOrNothingOption>(module, analytic_digital);
    auto integral_digital = bind_stateless_engine<QuadratureDigitalEngine>(
        module, "QuadratureDigitalEngine");
    bind_engine_price<QuadratureDigitalEngine, CashOrNothingOption>(module, integral_digital);
    bind_engine_price<QuadratureDigitalEngine, AssetOrNothingOption>(module, integral_digital);
    auto finite_digital = bind_finite_difference_engine<FiniteDifferenceDigitalEngine>(
        module, "FiniteDifferenceDigitalEngine", &make_finite_difference_digital_engine);
    bind_engine_price<FiniteDifferenceDigitalEngine, CashOrNothingOption>(module, finite_digital);
    bind_engine_price<FiniteDifferenceDigitalEngine, AssetOrNothingOption>(module, finite_digital);

    auto mc_barrier = nb::class_<MonteCarloBarrierEngine>(
                          module, "MonteCarloBarrierEngine",
                          "Single-barrier Monte Carlo pricing with native delta/gamma. Continuous monitoring uses exact terminal survival weights; scheduled monitoring uses actual observation dates. "
                          "Other Greeks are unavailable. Estimates have sampling error, especially when the first future observation is close. "
                          "CPU supports pseudorandom/Sobol; optional CUDA supports pseudorandom. All storage is request-local.")
                          .def(nb::new_([](PythonInteger path_count, PythonInteger step_count, PythonInteger seed,
                                           PythonChoice<MonteCarloBackend> backend, PythonInteger thread_count,
                                           PythonChoice<MonteCarloSampling> sampling, bool antithetic) {
                                   return unwrap(make_monte_carlo_barrier_engine(MonteCarloSettings{
                                       integer(path_count, "path_count"), integer(step_count, "step_count"), optional_seed(seed),
                                       backend, integer(thread_count, "thread_count"), sampling, antithetic}));
                               }),
                               nb::kw_only(), "path_count"_a = MonteCarloSettings{}.path_count,
                               "step_count"_a = MonteCarloSettings{}.step_count, "seed"_a.none() = MonteCarloSettings{}.seed,
                               "backend"_a = MonteCarloSettings{}.backend, "thread_count"_a = MonteCarloSettings{}.thread_count,
                               "sampling"_a = MonteCarloSettings{}.sampling, "antithetic"_a.noconvert() = MonteCarloSettings{}.antithetic,
                               R"doc(Create a barrier engine using the core MonteCarloSettings defaults.

path_count is the exact number of paths (1..10,000,000). step_count is validated
(2..10,000) but unused. seed is a nonnegative integer or None for system entropy.
backend is 'cpu' or 'cuda'; sampling is 'pseudo_random' or CPU-only 'sobol'.
Sobol requires an explicit power-of-two path_count. thread_count is a per-call CPU
cap (0 automatic, 1 serial); CUDA requires 0. antithetic pairs Z and -Z, leaving
one path unpaired for odd counts. Seeded CPU results reproduce across thread counts
in the same build; CPU and CUDA streams differ.
Invalid representations raise TypeError/OverflowError. Core configuration failures
raise KiyosiError (INVALID_PARAMETER or UNSUPPORTED_OPERATION for CUDA with Sobol).
Backend availability is checked when pricing. Operational failures follow the
package contract.)doc")
                          .def_prop_ro("path_count", [](const MonteCarloBarrierEngine& engine) { return engine.settings().path_count; })
                          .def_prop_ro("step_count", [](const MonteCarloBarrierEngine& engine) { return engine.settings().step_count; })
                          .def_prop_ro("seed", [](const MonteCarloBarrierEngine& engine) { return engine.settings().seed; })
                          .def_prop_ro("backend", [](const MonteCarloBarrierEngine& engine) { return engine.settings().backend; })
                          .def_prop_ro("thread_count", [](const MonteCarloBarrierEngine& engine) { return engine.settings().thread_count; })
                          .def_prop_ro("sampling", [](const MonteCarloBarrierEngine& engine) { return engine.settings().sampling; })
                          .def_prop_ro("antithetic", [](const MonteCarloBarrierEngine& engine) { return engine.settings().antithetic; });
    bind_repr(mc_barrier, "MonteCarloBarrierEngine", {"path_count", "step_count", "seed", "backend", "thread_count", "sampling", "antithetic"});
    bind_engine_price<MonteCarloBarrierEngine, BarrierOption>(module, mc_barrier);

    auto crr_barrier = nb::class_<CoxRossRubinsteinBarrierEngine>(
                           module, "CoxRossRubinsteinBarrierEngine",
                           "Single-barrier CRR pricing with node delta/gamma. Other Greeks are unavailable. Continuous monitoring is approximated at tree nodes; future scheduled dates must align exactly with tree steps, otherwise UNSUPPORTED_OPERATION is raised. "
                           "Price convergence can oscillate with strike alignment; gamma requires at least two steps.")
                           .def(nb::new_([](PythonInteger step_count) {
                                    return unwrap(make_cox_ross_rubinstein_barrier_engine({integer(step_count, "step_count")}));
                                }),
                                "step_count"_a = BinomialSettings{}.step_count,
                                "Create an engine with 1..1,000,000 steps. Invalid representations raise TypeError/OverflowError; invalid counts raise KiyosiError.")
                           .def_prop_ro("step_count", [](const CoxRossRubinsteinBarrierEngine& engine) { return engine.settings().step_count; });
    bind_repr(crr_barrier, "CoxRossRubinsteinBarrierEngine", {"step_count"});
    bind_engine_price<CoxRossRubinsteinBarrierEngine, BarrierOption>(module, crr_barrier);

    auto quadrature_barrier = bind_quadrature_engine<QuadratureBarrierEngine>(
        module, "QuadratureBarrierEngine", &make_quadrature_barrier_engine);
    bind_engine_price<QuadratureBarrierEngine, BarrierOption>(module, quadrature_barrier);

    auto analytic_barrier = bind_stateless_engine<AnalyticBarrierEngine>(
        module, "AnalyticBarrierEngine");
    bind_engine_price<AnalyticBarrierEngine, BarrierOption>(module, analytic_barrier);
    auto finite_barrier = bind_finite_difference_engine<FiniteDifferenceBarrierEngine>(
        module, "FiniteDifferenceBarrierEngine", &make_finite_difference_barrier_engine);
    bind_engine_price<FiniteDifferenceBarrierEngine, BarrierOption>(module, finite_barrier);
    auto finite_double_barrier = bind_finite_difference_engine<FiniteDifferenceDoubleBarrierEngine>(
        module, "FiniteDifferenceDoubleBarrierEngine", &make_finite_difference_double_barrier_engine);
    bind_engine_price<FiniteDifferenceDoubleBarrierEngine, DoubleBarrierOption>(module, finite_double_barrier);
    auto finite_double_touch = bind_finite_difference_engine<FiniteDifferenceDoubleTouchEngine>(
        module, "FiniteDifferenceDoubleTouchEngine", &make_finite_difference_double_touch_engine);
    bind_engine_price<FiniteDifferenceDoubleTouchEngine, DoubleTouchOption>(module, finite_double_touch);

    auto analytic_binary = bind_stateless_engine<AnalyticBinaryBarrierEngine>(
        module, "AnalyticBinaryBarrierEngine");
    bind_engine_price<AnalyticBinaryBarrierEngine, BinaryBarrierOption>(module, analytic_binary);
    bind_engine_price<AnalyticBinaryBarrierEngine, TouchOption>(module, analytic_binary);

    auto geometric = bind_stateless_engine<AnalyticGeometricAveragePriceEngine>(
        module, "AnalyticGeometricAveragePriceEngine");
    bind_engine_price<AnalyticGeometricAveragePriceEngine, GeometricAveragePriceOption>(module, geometric);
    auto arithmetic = bind_stateless_engine<TurnbullWakemanArithmeticAveragePriceEngine>(
        module, "TurnbullWakemanArithmeticAveragePriceEngine");
    bind_engine_price<TurnbullWakemanArithmeticAveragePriceEngine, ArithmeticAveragePriceOption>(module, arithmetic);

    auto finite_accumulator = bind_finite_difference_engine<FiniteDifferenceAccumulatorEngine>(
        module, "FiniteDifferenceAccumulatorEngine", &make_finite_difference_accumulator_engine);
    bind_engine_price<FiniteDifferenceAccumulatorEngine, Accumulator>(module, finite_accumulator);
    auto monte_carlo_accumulator = bind_trading_day_monte_carlo_engine<MonteCarloAccumulatorEngine>(
        module, "MonteCarloAccumulatorEngine", &make_monte_carlo_accumulator_engine);
    bind_engine_price<MonteCarloAccumulatorEngine, Accumulator>(module, monte_carlo_accumulator);

    auto quadrature_accumulator = bind_quadrature_engine<QuadratureAccumulatorEngine>(module, "QuadratureAccumulatorEngine", &make_quadrature_accumulator_engine);
    bind_engine_price<QuadratureAccumulatorEngine, Accumulator>(module, quadrature_accumulator);

    auto finite_autocallable = bind_finite_difference_engine<FiniteDifferenceAutocallableEngine>(module, "FiniteDifferenceAutocallableEngine", &make_finite_difference_autocallable_engine);
    bind_engine_price<FiniteDifferenceAutocallableEngine, SnowballOption>(module, finite_autocallable);
    bind_engine_price<FiniteDifferenceAutocallableEngine, BinarySnowballOption>(module, finite_autocallable);
    bind_engine_price<FiniteDifferenceAutocallableEngine, TernarySnowballOption>(module, finite_autocallable);
    bind_engine_price<FiniteDifferenceAutocallableEngine, PhoenixOption>(module, finite_autocallable);

    auto quadrature_autocallable = bind_quadrature_engine<QuadratureAutocallableEngine>(module, "QuadratureAutocallableEngine", &make_quadrature_autocallable_engine);
    bind_engine_price<QuadratureAutocallableEngine, SnowballOption>(module, quadrature_autocallable);
    bind_engine_price<QuadratureAutocallableEngine, BinarySnowballOption>(module, quadrature_autocallable);
    bind_engine_price<QuadratureAutocallableEngine, TernarySnowballOption>(module, quadrature_autocallable);
    bind_engine_price<QuadratureAutocallableEngine, PhoenixOption>(module, quadrature_autocallable);

    auto monte_carlo_autocallable = bind_trading_day_monte_carlo_engine<MonteCarloAutocallableEngine>(module, "MonteCarloAutocallableEngine", &make_monte_carlo_autocallable_engine);
    bind_engine_price<MonteCarloAutocallableEngine, SnowballOption>(module, monte_carlo_autocallable);
    bind_engine_price<MonteCarloAutocallableEngine, BinarySnowballOption>(module, monte_carlo_autocallable);
    bind_engine_price<MonteCarloAutocallableEngine, TernarySnowballOption>(module, monte_carlo_autocallable);
    bind_engine_price<MonteCarloAutocallableEngine, PhoenixOption>(module, monte_carlo_autocallable);
}

void bind_analytics(nb::module_& module)
{
    bind_snowball_implied_coupon_pair<QuadratureAutocallableEngine, SnowballOption>(module);
    bind_snowball_implied_coupon_pair<QuadratureAutocallableEngine, BinarySnowballOption>(module);
    bind_snowball_implied_coupon_pair<QuadratureAutocallableEngine, TernarySnowballOption>(module);
    bind_implied_coupon_pair<QuadratureAutocallableEngine, PhoenixOption>(module);
    bind_snowball_implied_coupon_pair<FiniteDifferenceAutocallableEngine, SnowballOption>(module);
    bind_snowball_implied_coupon_pair<MonteCarloAutocallableEngine, SnowballOption>(module);
    bind_snowball_implied_coupon_pair<FiniteDifferenceAutocallableEngine, BinarySnowballOption>(module);
    bind_snowball_implied_coupon_pair<MonteCarloAutocallableEngine, BinarySnowballOption>(module);
    bind_snowball_implied_coupon_pair<FiniteDifferenceAutocallableEngine, TernarySnowballOption>(module);
    bind_snowball_implied_coupon_pair<MonteCarloAutocallableEngine, TernarySnowballOption>(module);
    bind_implied_coupon_pair<FiniteDifferenceAutocallableEngine, PhoenixOption>(module);
    bind_implied_coupon_pair<MonteCarloAutocallableEngine, PhoenixOption>(module);
}

} // namespace kiyosi::python_binding
