#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <chrono>
#include <cmath>
#include <numbers>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"

namespace {

using kiyosi::test::day;
using kiyosi::test::risk_value;

TEST_CASE("Every engine treats Date expiry_date as a midnight instant", "[architecture]")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto midnight = kiyosi::start_of_day(expiry_date);
    const auto market = [&](kiyosi::Timestamp instant) {
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.03, 0.0, 0.2),
                                             110.0, instant);
    };
    const auto check = [&](const auto& engine, const auto& option, double terminal) {
        const auto settled = engine.price(option, market(midnight));
        REQUIRE(settled);
        CHECK(*settled == Catch::Approx(terminal));
        for (const auto delay : {std::chrono::seconds{1}, std::chrono::seconds{43200}}) {
            const auto expired = engine.price(option, market(midnight + delay));
            REQUIRE_FALSE(expired);
            CHECK(expired.error().category == kiyosi::ErrorCategory::invalid_time_range);
        }
    };
    const auto european = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, effective_date, expiry_date);
    const auto american = *kiyosi::make_american_option(
        kiyosi::OptionType::call, 100.0, effective_date, expiry_date);
    check(kiyosi::AnalyticVanillaEngine{}, european, 10.0);
    check(kiyosi::QuadratureVanillaEngine{}, european, 10.0);
    check(kiyosi::CoxRossRubinsteinVanillaEngine{32}, european, 10.0);
    check(kiyosi::CoxRossRubinsteinVanillaEngine{32}, american, 10.0);
    check(kiyosi::FiniteDifferenceVanillaEngine{}, european, 10.0);
    check(kiyosi::FiniteDifferenceVanillaEngine{}, american, 10.0);
    check(kiyosi::MonteCarloVanillaEngine{{32, 4, 7}}, european, 10.0);
    check(kiyosi::MonteCarloVanillaEngine{{32, 4, 7}}, american, 10.0);
    check(kiyosi::BjerksundStenslandVanillaEngine{}, american, 10.0);
    const auto digital = *kiyosi::make_cash_or_nothing_option(
        kiyosi::OptionType::call, 100.0, 7.0, effective_date, expiry_date);
    check(kiyosi::AnalyticDigitalEngine{}, digital, 7.0);
    check(kiyosi::QuadratureDigitalEngine{}, digital, 7.0);
    check(kiyosi::FiniteDifferenceDigitalEngine{}, digital, 7.0);
    const auto barrier = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                       .strike = 100.0,
                                                       .effective_date = effective_date,
                                                       .expiry_date = expiry_date,
                                                       .barrier_level = 80.0,
                                                       .barrier_type = kiyosi::BarrierType::down_and_out,
                                                       .touch_state = kiyosi::BarrierTouchState::untouched});
    check(kiyosi::AnalyticBarrierEngine{}, barrier, 10.0);
    check(kiyosi::FiniteDifferenceBarrierEngine{}, barrier, 10.0);
    const auto binary = *kiyosi::make_cash_binary_barrier_option(
        {.option_type = kiyosi::OptionType::call,
         .strike = 100.0,
         .effective_date = effective_date,
         .expiry_date = expiry_date,
         .barrier_level = 80.0,
         .barrier_type = kiyosi::BarrierType::down_and_out,
         .touch_state = kiyosi::BarrierTouchState::untouched},
        7.0);
    check(kiyosi::AnalyticBinaryBarrierEngine{}, binary, 7.0);
    check(kiyosi::AnalyticGeometricAveragePriceEngine{}, *kiyosi::make_geometric_average_option(kiyosi::OptionType::call, 100.0, effective_date, effective_date, expiry_date, 110.0), 10.0);
    check(kiyosi::TurnbullWakemanArithmeticAveragePriceEngine{}, *kiyosi::make_arithmetic_average_option(kiyosi::OptionType::call, 100.0, effective_date, effective_date, expiry_date, 110.0), 10.0);
    const auto note = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.1},
                                                            .maturity_coupon_rate = 0.05,
                                                            .knock_out_levels = {100.0},
                                                            .observation_dates = {expiry_date},
                                                            .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                            .principal_ratio = 1.0,
                                                            .effective_date = effective_date,
                                                            .expiry_date = expiry_date});
    check(kiyosi::MonteCarloBinarySnowballEngine{{32, 7}}, note, 1.1);
    check(kiyosi::FiniteDifferenceBinarySnowballEngine{}, note, 1.1);
    const auto accumulator = *kiyosi::make_accumulator({.strike = 100.0,
                                                        .knock_out_level = 120.0,
                                                        .daily_quantity = 1.0,
                                                        .acceleration_factor = 2.0,
                                                        .accumulated_quantity = 0.0,
                                                        .effective_date = effective_date,
                                                        .expiry_date = expiry_date});
    check(kiyosi::MonteCarloAccumulatorEngine{{32, 7}}, accumulator, 10.0);
    check(kiyosi::FiniteDifferenceAccumulatorEngine{}, accumulator, 10.0);
    const auto snowball = *kiyosi::make_snowball_option({.knock_out_coupon_rates = {0.1},
                                                         .maturity_coupon_rate = 0.05,
                                                         .initial_spot = 100.0,
                                                         .knock_in_level = 80.0,
                                                         .knock_out_levels = {100.0},
                                                         .upper_strike = 100.0,
                                                         .lower_strike = 60.0,
                                                         .observation_dates = {expiry_date},
                                                         .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry,
                                                         .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                         .principal_ratio = 1.0,
                                                         .effective_date = effective_date,
                                                         .expiry_date = expiry_date});
    check(kiyosi::MonteCarloSnowballEngine{{32, 7}}, snowball, 1.1);
    check(kiyosi::FiniteDifferenceSnowballEngine{}, snowball, 1.1);
    const auto ternary = *kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = {0.1},
                                                                .maturity_coupon_rate = 0.05,
                                                                .minimum_coupon_rate = 0.02,
                                                                .knock_in_level = 80.0,
                                                                .knock_out_levels = {100.0},
                                                                .observation_dates = {expiry_date},
                                                                .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry,
                                                                .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                                .principal_ratio = 1.0,
                                                                .effective_date = effective_date,
                                                                .expiry_date = expiry_date});
    check(kiyosi::MonteCarloTernarySnowballEngine{{32, 7}}, ternary, 1.1);
    check(kiyosi::FiniteDifferenceTernarySnowballEngine{}, ternary, 1.1);
    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.08,
                                                       .initial_spot = 100.0,
                                                       .knock_in_level = 80.0,
                                                       .knock_out_levels = {100.0},
                                                       .coupon_barrier_levels = {90.0},
                                                       .upper_strike = 100.0,
                                                       .lower_strike = 60.0,
                                                       .observation_dates = {expiry_date},
                                                       .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry,
                                                       .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                       .principal_ratio = 1.0,
                                                       .effective_date = effective_date,
                                                       .expiry_date = expiry_date});
    check(kiyosi::MonteCarloPhoenixEngine{{32, 7}}, phoenix, 1.08);
    check(kiyosi::FiniteDifferencePhoenixEngine{}, phoenix, 1.08);
}

TEST_CASE("Vanilla engines price the remaining half day", "[architecture]")
{
    const auto expiry_date = day(2026, 1, 1);
    const auto noon = kiyosi::start_of_day(expiry_date) - std::chrono::hours{12};
    const auto context = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.4),
                                                       100.0, noon);
    const auto option = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, kiyosi::date_of(noon), expiry_date);
    const double expected = 100.0 * std::erf(0.4 * std::sqrt(0.5 / 365.0) / (2.0 * std::numbers::sqrt2));
    const auto check = [&](const auto& engine, double tolerance) {
        const auto priced = engine.price(option, context);
        REQUIRE(priced);
        CHECK(*priced == Catch::Approx(expected).margin(tolerance));
    };
    check(kiyosi::AnalyticVanillaEngine{}, 1e-10);
    check(kiyosi::QuadratureVanillaEngine{}, 1e-6);
    check(kiyosi::CoxRossRubinsteinVanillaEngine{400}, 0.003);
    check(kiyosi::FiniteDifferenceVanillaEngine{{1000, 100}}, 0.02);
    check(kiyosi::MonteCarloVanillaEngine{{20000, 2, 7}}, 0.02);
}

struct RecordingEngine {
    std::vector<kiyosi::Timestamp>* moments{};
    kiyosi::Timestamp origin;

    kiyosi::Result<double> price(const kiyosi::EuropeanOption&,
                                 const kiyosi::PricingContext& context) const
    {
        moments->push_back(context.valuation_time());
        const double elapsed_days = std::chrono::duration<double, std::ratio<86400>>{
            context.valuation_time() - origin}
                                        .count();
        return context.model_parameters().volatility() + elapsed_days;
    }
};

TEST_CASE("Analytics preserve intraday valuation in market shifts", "[architecture]")
{
    const auto noon = kiyosi::start_of_day(day(2025, 7, 1)) + std::chrono::hours{12};
    const auto effective_date = day(2025, 1, 1);
    const auto context = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.03, 0.0, 0.2),
                                                       100.0, noon);
    const auto option = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, effective_date, day(2026, 1, 1));
    std::vector<kiyosi::Timestamp> moments;
    const RecordingEngine engine{&moments, noon};
    SECTION("numerical analytics")
    {
        const auto analytics = kiyosi::calculate_numerical_risk_measures(engine, option, context);
        REQUIRE(analytics);
        CHECK(risk_value(*analytics, kiyosi::RiskMeasure::vega) == Catch::Approx(0.01));
        CHECK(risk_value(*analytics, kiyosi::RiskMeasure::theta) == Catch::Approx(1.0));
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
    const auto effective_date = day(2025, 1, 1);
    const auto observation_date = day(2025, 7, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto note = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {10.0, 0.1},
                                                            .maturity_coupon_rate = 0.05,
                                                            .knock_out_levels = {90.0, 90.0},
                                                            .observation_dates = {observation_date, expiry_date},
                                                            .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                            .principal_ratio = 1.0,
                                                            .effective_date = effective_date,
                                                            .expiry_date = expiry_date});
    const auto check = [&](const auto& engine) {
        for (const int hour : {0, 12}) {
            const auto context = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 1e-12),
                                                               100.0, kiyosi::start_of_day(observation_date) + std::chrono::hours{hour});
            const auto priced = engine.price(note, context);
            REQUIRE(priced);
            const double expected = hour == 0
                                        ? 1.0 + 10.0 *
                                                    kiyosi::year_fraction(effective_date, observation_date).value()
                                        : 1.1;
            CHECK(*priced == Catch::Approx(expected).margin(1e-8));
        }
    };
    check(kiyosi::MonteCarloBinarySnowballEngine{{32, 7}});
    check(kiyosi::FiniteDifferenceBinarySnowballEngine{{100, 100}});
}

TEST_CASE("Daily knock-in observes midnight but not intraday spot", "[architecture]")
{
    const auto effective_date = day(2025, 1, 1);
    const auto observation_date = day(2025, 1, 2);
    const auto expiry_date = day(2025, 1, 3);
    const auto note = *kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = {0.1},
                                                             .maturity_coupon_rate = 0.8,
                                                             .minimum_coupon_rate = 0.2,
                                                             .knock_in_level = 80.0,
                                                             .knock_out_levels = {1000.0},
                                                             .observation_dates = {expiry_date},
                                                             .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                                             .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                             .principal_ratio = 1.0,
                                                             .effective_date = effective_date,
                                                             .expiry_date = expiry_date});
    const kiyosi::MonteCarloTernarySnowballEngine engine{{32, 7}};
    for (const int hour : {0, 12}) {
        const auto context = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(400.0, 0.0, 1e-12),
                                                           50.0, kiyosi::start_of_day(observation_date) + std::chrono::hours{hour});
        const auto priced = engine.price(note, context);
        REQUIRE(priced);
        const double coupon = hour == 0 ? 0.2 : 0.8;
        const double remaining = hour == 0 ? 1.0 / 365.0 : 0.5 / 365.0;
        const double expected = (1.0 + coupon * 2.0 / 365.0) * std::exp(-400.0 * remaining);
        CHECK(*priced == Catch::Approx(expected).margin(1e-10));
    }
}

} // namespace
