#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <chrono>
#include <limits>
#include <string>
#include <type_traits>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include <catch2/catch_approx.hpp>
#include "support/common.hpp"

namespace {

using kiyosi::test::day;
using kiyosi::test::risk_value;

TEST_CASE("Digital contracts validate and expose pricing results")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto cash_call = *kiyosi::make_cash_or_nothing_option(
        kiyosi::OptionType::call, 100.0, 10.0, valuation, expiry_date);
    const auto cash_put = *kiyosi::make_cash_or_nothing_option(
        kiyosi::OptionType::put, 100.0, 10.0, valuation, expiry_date);
    const kiyosi::AnalyticDigitalEngine digital;
    const auto call_value = digital.price(cash_call, context);
    const auto put_value = digital.price(cash_put, context);
    REQUIRE(call_value.has_value());
    REQUIRE(put_value.has_value());
    CHECK(call_value->has(kiyosi::RiskMeasure::price));
    CHECK(call_value->has(kiyosi::RiskMeasure::delta));
    CHECK(call_value->has(kiyosi::RiskMeasure::gamma));
    CHECK_FALSE(call_value->has(kiyosi::RiskMeasure::vega));
    CHECK_THAT(risk_value(*call_value, kiyosi::RiskMeasure::price) +
                   risk_value(*put_value, kiyosi::RiskMeasure::price),
               WithinAbs(10.0 * std::exp(-0.04), 1e-10));
    CHECK_FALSE(kiyosi::make_cash_or_nothing_option(
                    kiyosi::OptionType::call, 100.0, 0.0, valuation, expiry_date)
                    .has_value());
}

TEST_CASE("Barrier in and out prices compose to vanilla")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto down_out = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                        .strike = 100.0,
                                                        .effective_date = valuation,
                                                        .expiry_date = expiry_date,
                                                        .barrier_level = 90.0,
                                                        .barrier_type = kiyosi::BarrierType::down_and_out});
    const auto down_in = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                       .strike = 100.0,
                                                       .effective_date = valuation,
                                                       .expiry_date = expiry_date,
                                                       .barrier_level = 90.0,
                                                       .barrier_type = kiyosi::BarrierType::down_and_in});
    const auto barrier_out = kiyosi::AnalyticBarrierEngine{}.price(down_out, context);
    const auto barrier_in = kiyosi::AnalyticBarrierEngine{}.price(down_in, context);
    const auto vanilla = kiyosi::AnalyticVanillaEngine{}.price(
        *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date), context);
    REQUIRE(barrier_out.has_value());
    REQUIRE(barrier_in.has_value());
    REQUIRE(vanilla.has_value());
    CHECK_THAT(risk_value(*barrier_out, kiyosi::RiskMeasure::price) +
                   risk_value(*barrier_in, kiyosi::RiskMeasure::price),
               WithinAbs(risk_value(*vanilla, kiyosi::RiskMeasure::price), 1e-5));
}

TEST_CASE("Scheduled barrier contracts price analytically")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto scheduled = kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                        .strike = 100.0,
                                                        .effective_date = valuation,
                                                        .expiry_date = expiry_date,
                                                        .barrier_level = 90.0,
                                                        .barrier_type = kiyosi::BarrierType::down_and_out,
                                                        .rebate = 0.0,
                                                        .rebate_timing = kiyosi::RebateTiming::at_expiry,
                                                        .observation_mode = kiyosi::ObservationMode::scheduled,
                                                        .observation_dates = {valuation + std::chrono::days{30}}});
    REQUIRE(scheduled.has_value());
    CHECK(kiyosi::AnalyticBarrierEngine{}.price(*scheduled, context).has_value());
}

TEST_CASE("Deferred CPU instruments expose validated pricing paths")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = day(2025, 7, 1);
    auto parameters = kiyosi::make_bsm_parameters(0.03, 0.01, 0.2);
    auto context = kiyosi::make_pricing_context(*parameters, 100.0, valuation);
    REQUIRE(context.has_value());

    auto asian = kiyosi::make_geometric_average_option(
        kiyosi::OptionType::call, 100.0, valuation, valuation, expiry_date);
    REQUIRE(asian.has_value());
    auto asian_result = kiyosi::AnalyticGeometricAverageAsianEngine{}.price(*asian, *context);
    REQUIRE(asian_result.has_value());
    REQUIRE(asian_result->has(kiyosi::RiskMeasure::price));
    REQUIRE(*asian_result->require(kiyosi::RiskMeasure::price) > 0.0);

    auto note = kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.1},
                                                     .maturity_coupon_rate = 0.05,
                                                     .initial_spot = 100.0,
                                                     .knock_out_levels = {110.0},
                                                     .upper_strike = 100.0,
                                                     .lower_strike = 60.0,
                                                     .observation_dates = {expiry_date},
                                                     .touch_status = kiyosi::BarrierTouchStatus::none,
                                                     .principal_ratio = 1.0,
                                                     .effective_date = valuation,
                                                     .expiry_date = expiry_date});
    REQUIRE(note);
    kiyosi::MonteCarloBinarySnowballEngine engine{{128, 7}};
    auto note_result = engine.price(*note, *context);
    REQUIRE(note_result.has_value());
    REQUIRE(note_result->has(kiyosi::RiskMeasure::price));
}

TEST_CASE("Numerical analytics expose shared risk measures")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = day(2025, 7, 1);
    auto parameters = kiyosi::make_bsm_parameters(0.03, 0.01, 0.2);
    auto context = kiyosi::make_pricing_context(*parameters, 100.0, valuation);
    REQUIRE(context.has_value());
    auto option = kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    REQUIRE(option.has_value());
    auto analytics = kiyosi::calculate_numerical_analytics(kiyosi::CoxRossRubinsteinVanillaEngine{64}, *option, *context);
    REQUIRE(analytics.has_value());
    CHECK(analytics->has(kiyosi::RiskMeasure::speed));
    CHECK(analytics->has(kiyosi::RiskMeasure::rho));
    kiyosi::NumericalAnalyticsEngine<kiyosi::CoxRossRubinsteinVanillaEngine> shared{kiyosi::CoxRossRubinsteinVanillaEngine{64}};
    auto shared_result = shared.price(*option, *context);
    REQUIRE(shared_result.has_value());
    CHECK(shared_result->has(kiyosi::RiskMeasure::vega));
}

} // namespace
