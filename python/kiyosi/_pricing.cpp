#include "_binding.hpp"

#include <array>

using namespace nb::literals;

namespace kiyosi::python_binding {

namespace {

inline constexpr std::array<std::pair<const char*, risk_measure>, risk_measure_count> risk_measures{{
    {"price", risk_measure::price}, {"delta", risk_measure::delta},
    {"gamma", risk_measure::gamma}, {"speed", risk_measure::speed},
    {"theta", risk_measure::theta}, {"charm", risk_measure::charm},
    {"color", risk_measure::color}, {"vega", risk_measure::vega},
    {"vanna", risk_measure::vanna}, {"zomma", risk_measure::zomma},
    {"rho", risk_measure::rho},
}};

std::optional<risk_measure> measure_named(std::string_view name)
{
    for (const auto& [candidate, measure] : risk_measures)
        if (name == candidate) return measure;
    return std::nullopt;
}

PythonOptionalReal optional_value(const PricingResult& result, risk_measure measure)
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

template <typename Engine, typename Instrument>
void bind_engine_price(nb::class_<Engine>& binding)
{
    binding.def(
        "price",
        [](const Engine& engine, const Instrument& instrument, const PricingContext& context) {
            return unwrap(engine.price(instrument, context));
        },
        "instrument"_a, "context"_a, nb::call_guard<nb::gil_scoped_release>());
}

template <typename Engine>
nb::class_<Engine> bind_finite_difference_engine(nb::module_& module, const char* name)
{
    nb::class_<Engine> binding{module, name};
    binding
        .def(nb::new_([](PythonInteger asset_steps, PythonInteger time_steps,
                        finite_difference_scheme scheme, PythonReal upper_boundary) {
                 std::optional<double> boundary;
                 if (!upper_boundary.is_none())
                     boundary = real_number(upper_boundary, "upper_boundary");
                 return Engine{FiniteDifferenceSettings{
                     integer(asset_steps, "asset_steps"), integer(time_steps, "time_steps"),
                     scheme, boundary}};
             }),
             nb::kw_only(), "asset_steps"_a = FiniteDifferenceSettings{}.asset_steps,
             "time_steps"_a = FiniteDifferenceSettings{}.time_steps,
             "scheme"_a = FiniteDifferenceSettings{}.scheme,
             "upper_boundary"_a = nb::none())
        .def_prop_ro("asset_steps", [](const Engine& engine) { return engine.settings().asset_steps; })
        .def_prop_ro("time_steps", [](const Engine& engine) { return engine.settings().time_steps; })
        .def_prop_ro("scheme", [](const Engine& engine) { return engine.settings().scheme; })
        .def_prop_ro("upper_boundary", [](const Engine& engine) { return engine.settings().upper_boundary; });
    return binding;
}

template <typename Engine>
nb::class_<Engine> bind_structured_monte_carlo_engine(nb::module_& module, const char* name)
{
    nb::class_<Engine> binding{module, name};
    binding
        .def(nb::new_([](PythonInteger path_count, PythonInteger seed) {
                 return Engine{StructuredMonteCarloSettings{
                     integer(path_count, "path_count"), optional_seed(seed)}};
             }),
             nb::kw_only(), "path_count"_a = StructuredMonteCarloSettings{}.path_count,
             "seed"_a = StructuredMonteCarloSettings{}.seed.value())
        .def_prop_ro("path_count", [](const Engine& engine) { return engine.settings().path_count; })
        .def_prop_ro("seed", [](const Engine& engine) { return engine.settings().seed; });
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
        "numerical_analytics",
        [](const Engine& engine, const Instrument& instrument, const PricingContext& context,
           PythonReal spot_shift, PythonReal volatility_shift, PythonReal rate_shift,
           PythonInteger time_shift_days) {
            const auto settings = numerical_settings(
                spot_shift, volatility_shift, rate_shift, time_shift_days);
            nb::gil_scoped_release release;
            return unwrap(kiyosi::numerical_analytics(engine, instrument, context, settings));
        },
        "engine"_a, "instrument"_a, "context"_a, nb::kw_only(),
        "spot_shift"_a = NumericalShiftSettings{}.spot_shift,
        "volatility_shift"_a = NumericalShiftSettings{}.volatility_shift,
        "rate_shift"_a = NumericalShiftSettings{}.rate_shift,
        "time_shift_days"_a = NumericalShiftSettings{}.time_shift_days);
    module.def(
        "scenario_grid",
        [](const Engine& engine, const Instrument& instrument, const PricingContext& context,
           PythonRealSequence spots, PythonReal spot_shift, PythonReal volatility_shift,
           PythonReal rate_shift, PythonInteger time_shift_days) {
            const auto values = real_sequence(spots, "spots");
            const auto settings = numerical_settings(
                spot_shift, volatility_shift, rate_shift, time_shift_days);
            nb::gil_scoped_release release;
            return unwrap(kiyosi::scenario_grid(engine, instrument, context, values, settings));
        },
        "engine"_a, "instrument"_a, "context"_a, "spots"_a, nb::kw_only(),
        "spot_shift"_a = NumericalShiftSettings{}.spot_shift,
        "volatility_shift"_a = NumericalShiftSettings{}.volatility_shift,
        "rate_shift"_a = NumericalShiftSettings{}.rate_shift,
        "time_shift_days"_a = NumericalShiftSettings{}.time_shift_days);
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
        "max_iterations"_a = ImpliedVolatilitySettings{}.max_iterations);
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
        "max_iterations"_a = ImpliedCouponSettings{}.max_iterations);
}

template <typename Engine, typename... Instruments>
void bind_engine_analytics(nb::module_& module)
{
    (bind_analytics_pair<Engine, Instruments>(module), ...);
}

} // namespace

void bind_results(nb::module_& module)
{
    nb::class_<PricingResult>(module, "PricingResult")
        .def("__len__", [](const PricingResult&) { return risk_measure_count; })
        .def("__iter__", [](const PricingResult&) {
            return PythonStringIterator{result_keys().attr("__iter__")()};
        })
        .def("__contains__", [](const PricingResult&, nb::str key) {
            return measure_named(nb::cast<std::string>(key)).has_value();
        })
        .def("__getitem__", [](const PricingResult& result, nb::str key) {
            const std::string name = nb::cast<std::string>(key);
            const auto measure = measure_named(name);
            if (!measure) throw nb::key_error(name.c_str());
            return optional_value(result, *measure);
        })
        .def("get", [](const PricingResult& result, nb::str key, nb::object fallback) {
            const auto measure = measure_named(nb::cast<std::string>(key));
            return measure ? optional_value(result, *measure) : fallback;
        }, "key"_a, "default"_a = nb::none())
        .def("keys", [](const PricingResult&) { return result_keys(); })
        .def("values", [](const PricingResult& result) { return result_values(result); })
        .def("items", [](const PricingResult& result) { return result_items(result); })
        .def("require", [](const PricingResult& result, risk_measure measure) {
            return unwrap(result.require(measure));
        }, "measure"_a)
        .def_prop_ro("price", [](const PricingResult& result) {
            return optional_value(result, risk_measure::price);
        })
        .def_prop_ro("delta", [](const PricingResult& result) {
            return optional_value(result, risk_measure::delta);
        })
        .def_prop_ro("gamma", [](const PricingResult& result) {
            return optional_value(result, risk_measure::gamma);
        })
        .def_prop_ro("speed", [](const PricingResult& result) {
            return optional_value(result, risk_measure::speed);
        })
        .def_prop_ro("theta", [](const PricingResult& result) {
            return optional_value(result, risk_measure::theta);
        })
        .def_prop_ro("charm", [](const PricingResult& result) {
            return optional_value(result, risk_measure::charm);
        })
        .def_prop_ro("color", [](const PricingResult& result) {
            return optional_value(result, risk_measure::color);
        })
        .def_prop_ro("vega", [](const PricingResult& result) {
            return optional_value(result, risk_measure::vega);
        })
        .def_prop_ro("vanna", [](const PricingResult& result) {
            return optional_value(result, risk_measure::vanna);
        })
        .def_prop_ro("zomma", [](const PricingResult& result) {
            return optional_value(result, risk_measure::zomma);
        })
        .def_prop_ro("rho", [](const PricingResult& result) {
            return optional_value(result, risk_measure::rho);
        });
    nb::class_<ScenarioGridResult>(module, "ScenarioGridResult")
        .def_prop_ro("values", [](const ScenarioGridResult& result) { return result.values; })
        .def_prop_ro("deltas", [](const ScenarioGridResult& result) { return result.deltas; })
        .def_prop_ro("gammas", [](const ScenarioGridResult& result) { return result.gammas; });
    nb::module_::import_("collections.abc").attr("Mapping").attr("register")(
        module.attr("PricingResult"));
}

void bind_engines(nb::module_& module)
{
    auto analytic_vanilla = nb::class_<AnalyticVanillaEngine>(
        module, "AnalyticVanillaEngine").def(nb::init<>());
    bind_engine_price<AnalyticVanillaEngine, EuropeanOption>(analytic_vanilla);
    auto integral_vanilla = nb::class_<IntegralVanillaEngine>(
        module, "IntegralVanillaEngine").def(nb::init<>());
    bind_engine_price<IntegralVanillaEngine, EuropeanOption>(integral_vanilla);
    auto crr = nb::class_<CrrVanillaEngine>(module, "CrrVanillaEngine")
        .def(nb::new_([](PythonInteger steps) {
                 return CrrVanillaEngine{integer(steps, "steps")};
             }),
             "steps"_a = BinomialSettings{}.steps)
        .def_prop_ro("steps", [](const CrrVanillaEngine& engine) {
            return engine.settings().steps;
        });
    bind_engine_price<CrrVanillaEngine, EuropeanOption>(crr);
    bind_engine_price<CrrVanillaEngine, AmericanOption>(crr);
    auto bjerksund = nb::class_<BjerksundStenslandVanillaEngine>(
        module, "BjerksundStenslandVanillaEngine").def(nb::init<>());
    bind_engine_price<BjerksundStenslandVanillaEngine, AmericanOption>(bjerksund);
    auto finite_vanilla = bind_finite_difference_engine<FiniteDifferenceVanillaEngine>(
        module, "FiniteDifferenceVanillaEngine");
    bind_engine_price<FiniteDifferenceVanillaEngine, EuropeanOption>(finite_vanilla);
    bind_engine_price<FiniteDifferenceVanillaEngine, AmericanOption>(finite_vanilla);
    auto monte_carlo_vanilla = nb::class_<MonteCarloVanillaEngine>(
        module, "MonteCarloVanillaEngine")
        .def(nb::new_([](PythonInteger path_count, PythonInteger step_count,
                        PythonInteger seed) {
                 return MonteCarloVanillaEngine{MonteCarloSettings{
                     integer(path_count, "path_count"), integer(step_count, "step_count"),
                     optional_seed(seed)}};
             }),
             nb::kw_only(), "path_count"_a = MonteCarloSettings{}.path_count,
             "step_count"_a = MonteCarloSettings{}.step_count,
             "seed"_a = nb::none())
        .def_prop_ro("path_count", [](const MonteCarloVanillaEngine& engine) {
            return engine.settings().path_count;
        })
        .def_prop_ro("step_count", [](const MonteCarloVanillaEngine& engine) {
            return engine.settings().step_count;
        })
        .def_prop_ro("seed", [](const MonteCarloVanillaEngine& engine) {
            return engine.settings().seed;
        });
    bind_engine_price<MonteCarloVanillaEngine, EuropeanOption>(monte_carlo_vanilla);
    bind_engine_price<MonteCarloVanillaEngine, AmericanOption>(monte_carlo_vanilla);

    auto analytic_digital = nb::class_<AnalyticDigitalEngine>(
        module, "AnalyticDigitalEngine").def(nb::init<>());
    bind_engine_price<AnalyticDigitalEngine, EuropeanCashOrNothingOption>(analytic_digital);
    bind_engine_price<AnalyticDigitalEngine, EuropeanAssetOrNothingOption>(analytic_digital);
    auto integral_digital = nb::class_<IntegralDigitalEngine>(
        module, "IntegralDigitalEngine").def(nb::init<>());
    bind_engine_price<IntegralDigitalEngine, EuropeanCashOrNothingOption>(integral_digital);
    bind_engine_price<IntegralDigitalEngine, EuropeanAssetOrNothingOption>(integral_digital);
    auto finite_digital = bind_finite_difference_engine<FiniteDifferenceDigitalEngine>(
        module, "FiniteDifferenceDigitalEngine");
    bind_engine_price<FiniteDifferenceDigitalEngine, EuropeanCashOrNothingOption>(finite_digital);
    bind_engine_price<FiniteDifferenceDigitalEngine, EuropeanAssetOrNothingOption>(finite_digital);

    auto analytic_barrier = nb::class_<AnalyticBarrierEngine>(
        module, "AnalyticBarrierEngine").def(nb::init<>());
    bind_engine_price<AnalyticBarrierEngine, BarrierOption>(analytic_barrier);
    auto finite_barrier = bind_finite_difference_engine<FiniteDifferenceBarrierEngine>(
        module, "FiniteDifferenceBarrierEngine");
    bind_engine_price<FiniteDifferenceBarrierEngine, BarrierOption>(finite_barrier);
    auto analytic_binary = nb::class_<AnalyticBinaryBarrierEngine>(
        module, "AnalyticBinaryBarrierEngine").def(nb::init<>());
    bind_engine_price<AnalyticBinaryBarrierEngine, BinaryBarrierOption>(analytic_binary);

    auto geometric = nb::class_<GeometricAverageAsianEngine>(
        module, "GeometricAverageAsianEngine").def(nb::init<>());
    bind_engine_price<GeometricAverageAsianEngine, GeometricAverageOption>(geometric);
    auto arithmetic = nb::class_<ArithmeticAverageAsianEngine>(
        module, "ArithmeticAverageAsianEngine").def(nb::init<>());
    bind_engine_price<ArithmeticAverageAsianEngine, ArithmeticAverageOption>(arithmetic);

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
    bind_engine_analytics<IntegralVanillaEngine, EuropeanOption>(module);
    bind_engine_analytics<CrrVanillaEngine, EuropeanOption, AmericanOption>(module);
    bind_engine_analytics<BjerksundStenslandVanillaEngine, AmericanOption>(module);
    bind_engine_analytics<FiniteDifferenceVanillaEngine, EuropeanOption, AmericanOption>(module);
    bind_engine_analytics<MonteCarloVanillaEngine, EuropeanOption, AmericanOption>(module);
    bind_engine_analytics<AnalyticDigitalEngine, EuropeanCashOrNothingOption,
                          EuropeanAssetOrNothingOption>(module);
    bind_engine_analytics<IntegralDigitalEngine, EuropeanCashOrNothingOption,
                          EuropeanAssetOrNothingOption>(module);
    bind_engine_analytics<FiniteDifferenceDigitalEngine, EuropeanCashOrNothingOption,
                          EuropeanAssetOrNothingOption>(module);
    bind_engine_analytics<AnalyticBarrierEngine, BarrierOption>(module);
    bind_engine_analytics<FiniteDifferenceBarrierEngine, BarrierOption>(module);
    bind_engine_analytics<AnalyticBinaryBarrierEngine, BinaryBarrierOption>(module);
    bind_engine_analytics<GeometricAverageAsianEngine, GeometricAverageOption>(module);
    bind_engine_analytics<ArithmeticAverageAsianEngine, ArithmeticAverageOption>(module);
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
    bind_implied_coupon_pair<FiniteDifferenceSnowballEngine, SnowballOption>(module);
    bind_implied_coupon_pair<MonteCarloSnowballEngine, SnowballOption>(module);
    bind_implied_coupon_pair<FiniteDifferencePhoenixEngine, PhoenixOption>(module);
    bind_implied_coupon_pair<MonteCarloPhoenixEngine, PhoenixOption>(module);
}

} // namespace kiyosi::python_binding
