#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <chrono>
#include <cmath>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"

namespace {

using kiyosi::test::day;
using kiyosi::test::risk_value;

TEST_CASE("Every engine treats date expiry as a midnight instant", "[architecture]")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto midnight = kiyosi::start_of_day(expiry);
    const auto market = [&](kiyosi::timestamp instant) {
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.03, 0.0, 0.2),
                                             *kiyosi::make_asset_price(110.0), instant);
    };
    const auto check = [&](const auto& engine, const auto& option, double terminal) {
        const auto settled = engine.price(option, market(midnight));
        REQUIRE(settled);
        CHECK(risk_value(*settled, kiyosi::risk_measure::price) == Catch::Approx(terminal));
        for (const auto delay : {std::chrono::seconds{1}, std::chrono::seconds{43200}}) {
            const auto expired = engine.price(option, market(midnight + delay));
            REQUIRE_FALSE(expired);
            CHECK(expired.error().category == kiyosi::error_category::invalid_expiry);
        }
    };
    const auto european = *kiyosi::make_european_call(100.0, expiry);
    const auto american = *kiyosi::make_american_call(100.0, expiry);
    check(kiyosi::AnalyticEuropeanEngine{}, european, 10.0);
    check(kiyosi::IntegralEuropeanEngine{}, european, 10.0);
    check(kiyosi::BinomialEuropeanEngine{32}, european, 10.0);
    check(kiyosi::BinomialAmericanEngine{32}, american, 10.0);
    check(kiyosi::FiniteDifferenceEuropeanEngine{}, european, 10.0);
    check(kiyosi::FiniteDifferenceAmericanEngine{}, american, 10.0);
    check(kiyosi::MonteCarloEuropeanEngine{{32, 4, 7}}, european, 10.0);
    check(kiyosi::MonteCarloAmericanEngine{{32, 4, 7}}, american, 10.0);
    check(kiyosi::BjerksundStenslandAmericanEngine{}, american, 10.0);
    const auto digital = *kiyosi::make_cash_or_nothing_option(kiyosi::option_type::call, 100.0, 7.0, expiry);
    check(kiyosi::AnalyticDigitalEngine{}, digital, 7.0);
    check(kiyosi::IntegralDigitalEngine{}, digital, 7.0);
    check(kiyosi::FiniteDifferenceDigitalEngine{}, digital, 7.0);
    const auto barrier = *kiyosi::make_barrier_option(
        kiyosi::option_type::call, 100.0, effective, expiry, 80.0, kiyosi::barrier_type::down_and_out);
    check(kiyosi::AnalyticBarrierEngine{}, barrier, 10.0);
    check(kiyosi::FiniteDifferenceBarrierEngine{}, barrier, 10.0);
    const auto binary = *kiyosi::make_cash_or_nothing_barrier_option(
        kiyosi::option_type::call, 100.0, effective, expiry, 80.0, kiyosi::barrier_type::down_and_out, 7.0);
    check(kiyosi::AnalyticBinaryBarrierEngine{}, binary, 7.0);
    check(kiyosi::GeometricAverageAsianEngine{}, *kiyosi::make_geometric_average_option(kiyosi::option_type::call, 100.0, effective, expiry, 110.0), 10.0);
    check(kiyosi::ArithmeticAverageAsianEngine{}, *kiyosi::make_arithmetic_average_option(kiyosi::option_type::call, 100.0, effective, expiry, 110.0), 10.0);
    const auto note = *kiyosi::make_binary_snowball_option(
        {0.1}, 0.05, 100.0, {100.0}, 100.0, 60.0, {expiry},
        kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    check(kiyosi::MonteCarloBinarySnowballEngine{{32, 7}}, note, 1.1);
    check(kiyosi::FiniteDifferenceBinarySnowballEngine{}, note, 1.1);
    const auto accumulator = *kiyosi::make_accumulator(100.0, 120.0, 1.0, 2.0, 0.0, effective, expiry);
    check(kiyosi::MonteCarloAccumulatorEngine{{32, 7}}, accumulator, 10.0);
    check(kiyosi::FiniteDifferenceAccumulatorEngine{}, accumulator, 10.0);
    const auto snowball = *kiyosi::make_snowball_option(
        {0.1}, 0.05, 100.0, 80.0, {100.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::at_expiry, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    check(kiyosi::MonteCarloSnowballEngine{{32, 7}}, snowball, 1.1);
    check(kiyosi::FiniteDifferenceSnowballEngine{}, snowball, 1.1);
    const auto ternary = *kiyosi::make_ternary_snowball_option(
        {0.1}, 0.05, 0.02, 100.0, 80.0, {100.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::at_expiry, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    check(kiyosi::MonteCarloTernarySnowballEngine{{32, 7}}, ternary, 1.1);
    check(kiyosi::FiniteDifferenceTernarySnowballEngine{}, ternary, 1.1);
    const auto phoenix = *kiyosi::make_phoenix_option(
        0.08, 100.0, 80.0, {100.0}, {90.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::at_expiry, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    check(kiyosi::MonteCarloPhoenixEngine{{32, 7}}, phoenix, 9.0);
    check(kiyosi::FiniteDifferencePhoenixEngine{}, phoenix, 9.0);
}

TEST_CASE("Vanilla engines price the remaining half day", "[architecture]")
{
    const auto expiry = day(2026, 1, 1);
    const auto noon = kiyosi::start_of_day(expiry) - std::chrono::hours{12};
    const auto context = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.4),
                                                       *kiyosi::make_asset_price(100.0), noon);
    const auto option = *kiyosi::make_european_call(100.0, expiry);
    const double expected = 100.0 * std::erf(0.4 * std::sqrt(0.5 / 365.0) / (2.0 * std::sqrt(2.0)));
    const auto check = [&](const auto& engine, double tolerance) {
        const auto priced = engine.price(option, context);
        REQUIRE(priced);
        CHECK(risk_value(*priced, kiyosi::risk_measure::price) == Catch::Approx(expected).margin(tolerance));
    };
    check(kiyosi::AnalyticEuropeanEngine{}, 1e-10);
    check(kiyosi::IntegralEuropeanEngine{}, 1e-6);
    check(kiyosi::BinomialEuropeanEngine{400}, 0.003);
    check(kiyosi::FiniteDifferenceEuropeanEngine{{1000, 100}}, 0.02);
    check(kiyosi::MonteCarloEuropeanEngine{{20000, 2, 7}}, 0.02);
}

struct RecordingEngine {
    std::vector<kiyosi::timestamp>& moments;
    kiyosi::timestamp origin;

    kiyosi::result<kiyosi::PricingResult> price(const kiyosi::EuropeanOption&,
                                                const kiyosi::PricingContext& context) const
    {
        moments.push_back(context.valuation_time());
        const double elapsed_days = std::chrono::duration<double, std::ratio<86400>>{
            context.valuation_time() - origin}
                                        .count();
        return kiyosi::PricingResult{{kiyosi::risk_measure::price,
                                      context.parameters().volatility() + elapsed_days}};
    }
};

TEST_CASE("Analytics preserve intraday valuation in market shifts", "[architecture]")
{
    const auto noon = kiyosi::start_of_day(day(2025, 7, 1)) + std::chrono::hours{12};
    const auto context = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.03, 0.0, 0.2),
                                                       *kiyosi::make_asset_price(100.0), noon);
    const auto option = *kiyosi::make_european_call(100.0, day(2026, 1, 1));
    std::vector<kiyosi::timestamp> moments;
    const RecordingEngine engine{moments, noon};
    SECTION("numerical analytics and scenarios")
    {
        const auto analytics = kiyosi::numerical_analytics(engine, option, context);
        REQUIRE(analytics);
        CHECK(risk_value(*analytics, kiyosi::risk_measure::vega) == Catch::Approx(0.01));
        CHECK(risk_value(*analytics, kiyosi::risk_measure::theta) == Catch::Approx(1.0));
        const auto scenarios = kiyosi::scenario_grid(engine, option, context, std::vector<double>{90.0, 110.0});
        REQUIRE(scenarios);
        CHECK(scenarios->values == std::vector<double>{0.2, 0.2});
        REQUIRE_FALSE(moments.empty());
        for (const auto moment : moments) {
            CHECK(moment - kiyosi::start_of_day(kiyosi::date_of(moment)) == std::chrono::hours{12});
            CHECK((moment == noon || moment == noon - std::chrono::days{1} || moment == noon + std::chrono::days{1}));
        }
    }
    SECTION("implied volatility")
    {
        const auto volatility = kiyosi::implied_volatility(engine, option, context, 0.3);
        REQUIRE(volatility);
        CHECK(*volatility == Catch::Approx(0.3).margin(1e-8));
        REQUIRE_FALSE(moments.empty());
        for (const auto moment : moments)
            CHECK(moment == noon);
    }
}

TEST_CASE("Structured observations occur at midnight only", "[architecture]")
{
    const auto effective = day(2025, 1, 1);
    const auto observation = day(2025, 7, 1);
    const auto expiry = day(2026, 1, 1);
    const auto note = *kiyosi::make_binary_snowball_option(
        {10.0, 0.1}, 0.05, 100.0, {90.0, 90.0}, 100.0, 60.0, {observation, expiry},
        kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto check = [&](const auto& engine) {
        for (const int hour : {0, 12}) {
            const auto context = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 1e-12),
                                                               *kiyosi::make_asset_price(100.0), kiyosi::start_of_day(observation) + std::chrono::hours{hour});
            const auto priced = engine.price(note, context);
            REQUIRE(priced);
            const double expected = hour == 0 ? 1.0 + 10.0 * kiyosi::year_fraction(effective, observation).value() : 1.1;
            CHECK(risk_value(*priced, kiyosi::risk_measure::price) == Catch::Approx(expected).margin(1e-8));
        }
    };
    check(kiyosi::MonteCarloBinarySnowballEngine{{32, 7}});
    check(kiyosi::FiniteDifferenceBinarySnowballEngine{{100, 100}});
}

TEST_CASE("Daily knock-in observes midnight but not intraday spot", "[architecture]")
{
    const auto effective = day(2025, 1, 1);
    const auto observation = day(2025, 1, 2);
    const auto expiry = day(2025, 1, 3);
    const auto note = *kiyosi::make_ternary_snowball_option(
        {0.1}, 0.8, 0.2, 100.0, 80.0, {1000.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    const kiyosi::MonteCarloTernarySnowballEngine engine{{32, 7}};
    for (const int hour : {0, 12}) {
        const auto context = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(400.0, 0.0, 1e-12),
            *kiyosi::make_asset_price(50.0), kiyosi::start_of_day(observation) + std::chrono::hours{hour});
        const auto priced = engine.price(note, context);
        REQUIRE(priced);
        const double coupon = hour == 0 ? 0.2 : 0.8;
        const double remaining = hour == 0 ? 1.0 / 365.0 : 0.5 / 365.0;
        const double expected = (1.0 + coupon * 2.0 / 365.0) * std::exp(-400.0 * remaining);
        CHECK(risk_value(*priced, kiyosi::risk_measure::price) == Catch::Approx(expected).margin(1e-10));
    }
}

} // namespace
