#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <array>
#include <limits>
#include <type_traits>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"

namespace {
using namespace kiyosi;
using kiyosi::test::day;
using kiyosi::test::risk_value;
const auto effective = day(2025, 1, 1);
const auto valuation = day(2025, 7, 1);
const auto expiry = day(2026, 1, 1);

PricingContext market(double spot = 100.0, Date date = valuation)
{
    return *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.3), spot, date);
}

struct RecordingPriceEngine {
    std::vector<PricingContext>& calls;
    bool reject_bump = false;
    Result<double> price(const EuropeanOption&, const PricingContext& context) const
    {
        calls.push_back(context);
        if (reject_bump && context.spot_price() != 100.0)
            return std::unexpected(Error{ErrorCategory::invalid_schedule, "bump rejected"});
        return context.spot_price() * context.spot_price();
    }
};

template <typename Engine>
concept implicit_greeks_level = requires(const Engine& engine, const EuropeanOption& option,
                                         const PricingContext& context) {
    engine.price_with_greeks(option, context);
};
static_assert(!implicit_greeks_level<AnalyticVanillaEngine>);
static_assert(std::is_same_v<decltype(AnalyticVanillaEngine{}.price(
    std::declval<const EuropeanOption&>(), std::declval<const PricingContext&>())), Result<double>>);

TEST_CASE("Pricing API separates scalar basic and full outputs", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    const auto context = market();
    const AnalyticVanillaEngine engine;
    const auto scalar = engine.price(option, context);
    const auto basic = engine.price_with_greeks(option, context, GreeksLevel::basic);
    const auto full = engine.price_with_greeks(option, context, GreeksLevel::full);
    REQUIRE(scalar);
    REQUIRE(basic);
    REQUIRE(full);
    CHECK(risk_value(*basic, RiskMeasure::price) == *scalar);
    CHECK(risk_value(*full, RiskMeasure::price) == *scalar);
    CHECK(risk_value(*basic, RiskMeasure::delta) == risk_value(*full, RiskMeasure::delta));
    CHECK(risk_value(*basic, RiskMeasure::gamma) == risk_value(*full, RiskMeasure::gamma));
    for (std::size_t i = 1; i < risk_measure_count; ++i) {
        const auto measure = static_cast<RiskMeasure>(i);
        CAPTURE(i);
        CHECK(full->has(measure));
        CHECK(basic->has(measure) == (measure == RiskMeasure::delta || measure == RiskMeasure::gamma));
    }
}

TEST_CASE("Basic Greek completion uses only missing spot differences", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    const auto context = market();
    std::vector<PricingContext> calls;
    const RecordingPriceEngine engine{calls};
    const auto native = *make_pricing_result({{RiskMeasure::price, 10000.0}});
    const auto basic = detail::complete_greeks(engine, option, context, GreeksLevel::basic, {}, native);
    REQUIRE(basic);
    REQUIRE(calls.size() == 2);
    CHECK(calls[0].spot_price() == 100.0 + NumericalShiftSettings{}.spot_shift);
    CHECK(calls[1].spot_price() == 100.0 - NumericalShiftSettings{}.spot_shift);
    for (const auto& call : calls) {
        CHECK(call.valuation_time() == context.valuation_time());
        CHECK(call.model_parameters().volatility() == 0.3);
        CHECK(call.model_parameters().risk_free_rate() == 0.04);
    }
    CHECK(risk_value(*basic, RiskMeasure::delta) == Catch::Approx(200.0));
    CHECK(risk_value(*basic, RiskMeasure::gamma) == Catch::Approx(2.0).margin(1e-5));
    calls.clear();
    const auto supplied = *make_pricing_result({{RiskMeasure::price, 10000.0},
                                               {RiskMeasure::delta, 17.0}, {RiskMeasure::gamma, 0.0}});
    const auto preserved = detail::complete_greeks(engine, option, context, GreeksLevel::basic, {}, supplied);
    REQUIRE(preserved);
    CHECK(calls.empty());
    CHECK(risk_value(*preserved, RiskMeasure::delta) == 17.0);
    CHECK(risk_value(*preserved, RiskMeasure::gamma) == 0.0);
    const auto full = detail::complete_greeks(engine, option, context, GreeksLevel::full, {}, supplied);
    REQUIRE(full);
    CHECK_FALSE(calls.empty());
    CHECK(risk_value(*full, RiskMeasure::delta) == 17.0);
    CHECK(risk_value(*full, RiskMeasure::gamma) == 0.0);
    CHECK(risk_value(*full, RiskMeasure::vega) == 0.0);
    CHECK(risk_value(*full, RiskMeasure::rho) == 0.0);
}

TEST_CASE("Joint pricing validates discriminators and every shift", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    const AnalyticVanillaEngine engine;
    const auto invalid_level = engine.price_with_greeks(option, market(), static_cast<GreeksLevel>(99));
    REQUIRE_FALSE(invalid_level);
    CHECK(invalid_level.error().category == ErrorCategory::invalid_parameter);
    for (const auto shifts : {NumericalShiftSettings{0.0},
                              NumericalShiftSettings{.volatility_shift = -0.1},
                              NumericalShiftSettings{.rate_shift = std::numeric_limits<double>::infinity()},
                              NumericalShiftSettings{.time_shift_days = 0}}) {
        const auto result = engine.price_with_greeks(option, market(), GreeksLevel::basic, shifts);
        REQUIRE_FALSE(result);
        CHECK(result.error().category == ErrorCategory::invalid_parameter);
    }
}

TEST_CASE("Joint completion keeps unavailable stencils and propagates feasible failures", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    std::vector<PricingContext> calls;
    const auto native = *make_pricing_result({{RiskMeasure::price, 10000.0}});
    const auto unavailable = detail::complete_greeks(RecordingPriceEngine{calls}, option,
        market(), GreeksLevel::basic, NumericalShiftSettings{.spot_shift = 100.0}, native);
    REQUIRE(unavailable);
    CHECK(risk_value(*unavailable, RiskMeasure::price) == 10000.0);
    CHECK_FALSE(unavailable->has(RiskMeasure::delta));
    CHECK_FALSE(unavailable->has(RiskMeasure::gamma));
    CHECK(calls.empty());
    const auto rejected = detail::complete_greeks(RecordingPriceEngine{calls, true}, option,
        market(), GreeksLevel::basic, {}, native);
    REQUIRE_FALSE(rejected);
    CHECK(rejected.error().category == ErrorCategory::invalid_schedule);
    CHECK(rejected.error().message == "bump rejected");
    CHECK(calls.size() == 1);
}

TEST_CASE("Expiry suppresses all Greeks for every requested tier", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    const auto check = [&](const auto& engine) {
        for (const auto level : {GreeksLevel::basic, GreeksLevel::full}) {
            const auto result = engine.price_with_greeks(option, market(110.0, expiry), level);
            REQUIRE(result);
            CHECK(risk_value(*result, RiskMeasure::price) == 10.0);
            for (std::size_t i = 1; i < risk_measure_count; ++i)
                CHECK_FALSE(result->has(static_cast<RiskMeasure>(i)));
        }
    };
    check(AnalyticVanillaEngine{});
    check(QuadratureVanillaEngine{});
    check(CoxRossRubinsteinVanillaEngine{8});
    check(FiniteDifferenceVanillaEngine{20, 20});
    check(MonteCarloVanillaEngine{32, 2, 7});
}

TEST_CASE("Monitored barrier equality leaves all Greeks unavailable", "[pricing-api]")
{
    const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0,
        .effective_date = effective, .expiry_date = expiry, .barrier_level = 100.0,
        .barrier_type = BarrierType::down_and_out, .rebate = 2.0});
    const AnalyticBarrierEngine engine;
    const auto scalar = engine.price(option, market());
    REQUIRE(scalar);
    for (const auto level : {GreeksLevel::basic, GreeksLevel::full}) {
        const auto result = engine.price_with_greeks(option, market(), level);
        REQUIRE(result);
        CHECK(risk_value(*result, RiskMeasure::price) == *scalar);
        for (std::size_t i = 1; i < risk_measure_count; ++i)
            CHECK_FALSE(result->has(static_cast<RiskMeasure>(i)));
    }
}

TEST_CASE("Extreme time shift is bounded before timestamp arithmetic", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    std::vector<PricingContext> calls;
    const auto result = calculate_numerical_risk_measures(RecordingPriceEngine{calls}, option,
        market(), NumericalShiftSettings{.time_shift_days = std::numeric_limits<int>::max()});
    REQUIRE(result);
    CHECK(risk_value(*result, RiskMeasure::theta) == 0.0);
    for (const auto& call : calls) {
        CHECK(call.valuation_time() >= start_of_day(effective));
        CHECK(call.valuation_time() <= start_of_day(expiry));
    }
}

TEST_CASE("Seeded Monte Carlo joint pricing is reproducible and matches scalar pricing", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    const MonteCarloVanillaEngine engine{256, 4, 73};
    const auto scalar = engine.price(option, market());
    REQUIRE(scalar);
    for (const auto level : {GreeksLevel::basic, GreeksLevel::full}) {
        const auto first = engine.price_with_greeks(option, market(), level);
        const auto second = engine.price_with_greeks(option, market(), level);
        REQUIRE(first);
        REQUIRE(second);
        CHECK(risk_value(*first, RiskMeasure::price) == *scalar);
        for (std::size_t i = 0; i < risk_measure_count; ++i)
            CHECK(first->values_view()[i] == second->values_view()[i]);
        CHECK(risk_value(*first, RiskMeasure::delta) >= 0.0);
        CHECK(risk_value(*first, RiskMeasure::delta) <= 1.2);
    }
    CHECK(engine.settings().seed == 73);
}
TEST_CASE("Scalar and basic analytic pricing avoid overflowing higher Greeks", "[pricing-api]")
{
    const auto option = *kiyosi::make_european_option(kiyosi::OptionType::call, 1e-160,
        effective, expiry);
    const kiyosi::AnalyticVanillaEngine engine;
    const auto context = market(1e-160);
    const auto scalar = engine.price(option, context);
    const auto basic = engine.price_with_greeks(option, context, kiyosi::GreeksLevel::basic);
    const auto full = engine.price_with_greeks(option, context, kiyosi::GreeksLevel::full);
    REQUIRE(scalar);
    REQUIRE(basic);
    CHECK(*scalar > 0.0);
    CHECK(risk_value(*basic, kiyosi::RiskMeasure::price) == *scalar);
    CHECK(risk_value(*basic, kiyosi::RiskMeasure::gamma) > 1e150);
    REQUIRE_FALSE(full);
    CHECK(full.error().category == kiyosi::ErrorCategory::invalid_result);
}
struct SeedRecordingSettings {
    std::optional<unsigned> seed;
    std::vector<std::optional<unsigned>>* calls;
};
struct SeedRecordingEngine {
    SeedRecordingSettings configuration;
    const SeedRecordingSettings& settings() const { return configuration; }
    kiyosi::Result<double> price(const kiyosi::EuropeanOption&,
                                  const kiyosi::PricingContext& context) const
    {
        configuration.calls->push_back(configuration.seed);
        return context.spot_price() * context.spot_price();
    }
};

TEST_CASE("Unseeded joint pricing selects one seed without changing the engine", "[pricing-api]")
{
    const auto option = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective, expiry);
    std::vector<std::optional<unsigned>> calls;
    const SeedRecordingEngine engine{{std::nullopt, &calls}};
    const auto result = kiyosi::detail::price_with_greeks(engine, option, market(),
        kiyosi::GreeksLevel::basic, {}, [&](const auto& seeded) -> kiyosi::Result<kiyosi::PricingResult> {
            const auto value = seeded.price(option, market());
            if (!value) return std::unexpected(value.error());
            return kiyosi::make_pricing_result({{kiyosi::RiskMeasure::price, *value}});
        });
    REQUIRE(result);
    REQUIRE(calls.size() == 3);
    REQUIRE(calls.front().has_value());
    for (const auto seed : calls) CHECK(seed == calls.front());
    CHECK_FALSE(engine.settings().seed.has_value());
    CHECK(risk_value(*result, kiyosi::RiskMeasure::delta) == Catch::Approx(200.0));
}

struct SolverSeedRecordingEngine {
    SeedRecordingSettings configuration;
    const SeedRecordingSettings& settings() const { return configuration; }
    kiyosi::Result<double> price(const kiyosi::EuropeanOption&, const kiyosi::PricingContext& context) const
    {
        configuration.calls->push_back(configuration.seed);
        return context.model_parameters().volatility();
    }
    kiyosi::Result<double> price(const kiyosi::PhoenixOption& option, const kiyosi::PricingContext&) const
    {
        configuration.calls->push_back(configuration.seed);
        return option.coupon_rate();
    }
};

TEST_CASE("Implied solvers keep one seed for every trial without changing the engine", "[pricing-api]")
{
    const auto option = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective, expiry);
    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.05, .initial_spot = 100.0,
        .knock_in_level = 80.0, .knock_out_levels = {120.0}, .coupon_barrier_levels = {90.0},
        .upper_strike = 100.0, .lower_strike = 60.0, .observation_dates = {expiry},
        .effective_date = effective, .expiry_date = expiry});
    std::vector<std::optional<unsigned>> calls;
    const SolverSeedRecordingEngine engine{{std::nullopt, &calls}};
    const auto check_calls = [&] {
        REQUIRE(calls.size() > 2);
        REQUIRE(calls.front().has_value());
        for (const auto seed : calls) CHECK(seed == calls.front());
        calls.clear();
    };

    const auto volatility = kiyosi::implied_volatility(engine, option, market(), 0.3);
    REQUIRE(volatility);
    CHECK(*volatility == Catch::Approx(0.3).margin(1e-8));
    check_calls();

    const auto coupon = kiyosi::implied_coupon(engine, phoenix, market(), 0.125);
    REQUIRE(coupon);
    CHECK(*coupon == Catch::Approx(0.125).margin(1e-8));
    check_calls();
    CHECK_FALSE(engine.settings().seed.has_value());

    const SolverSeedRecordingEngine seeded{{73, &calls}};
    REQUIRE(kiyosi::implied_volatility(seeded, option, market(), 0.3));
    REQUIRE(calls.size() > 2);
    for (const auto seed : calls) CHECK(seed == 73);
}
TEST_CASE("Event thresholds suppress spot Greeks but retain rate and volatility sensitivities", "[pricing-api]")
{
    const auto accumulator = *kiyosi::make_accumulator({.strike = 90.0, .knock_out_level = 100.0,
        .daily_quantity = 1.0, .acceleration_factor = 2.0, .accumulated_quantity = 3.0,
        .effective_date = effective, .expiry_date = expiry});
    const auto snowball = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.1, 0.1},
        .maturity_coupon_rate = 0.05, .initial_spot = 100.0, .knock_out_levels = {100.0, 100.0},
        .upper_strike = 100.0, .lower_strike = 60.0, .observation_dates = {valuation, expiry},
        .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0,
        .effective_date = effective, .expiry_date = expiry});
    const auto check = [&](const auto& engine, const auto& option) {
        const auto result = engine.price_with_greeks(option, market(), kiyosi::GreeksLevel::full);
        REQUIRE(result);
        CHECK(risk_value(*result, kiyosi::RiskMeasure::price) == *engine.price(option, market()));
        CHECK(risk_value(*result, kiyosi::RiskMeasure::vega) == 0.0);
        CHECK(risk_value(*result, kiyosi::RiskMeasure::rho) == 0.0);
        for (const auto measure : {kiyosi::RiskMeasure::delta, kiyosi::RiskMeasure::gamma,
            kiyosi::RiskMeasure::speed, kiyosi::RiskMeasure::theta, kiyosi::RiskMeasure::charm,
            kiyosi::RiskMeasure::color, kiyosi::RiskMeasure::vanna, kiyosi::RiskMeasure::zomma})
            CHECK_FALSE(result->has(measure));
        const auto noon = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.04, 0.01, 0.3),
            100.0, kiyosi::start_of_day(valuation) + std::chrono::hours{12});
        const auto after = engine.price_with_greeks(option, noon, kiyosi::GreeksLevel::basic);
        REQUIRE(after);
        REQUIRE(after->has(kiyosi::RiskMeasure::delta));
        REQUIRE(after->has(kiyosi::RiskMeasure::gamma));
        CHECK(std::isfinite(risk_value(*after, kiyosi::RiskMeasure::delta)));
        CHECK(std::isfinite(risk_value(*after, kiyosi::RiskMeasure::gamma)));
    };
    check(kiyosi::MonteCarloAccumulatorEngine{{64, 7}}, accumulator);
    check(kiyosi::MonteCarloBinarySnowballEngine{{64, 7}}, snowball);
}

} // namespace
