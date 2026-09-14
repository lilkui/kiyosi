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
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto cash_call = *kiyosi::make_cash_or_nothing_option(
        kiyosi::option_type::call, 100.0, 10.0, valuation, expiry);
    const auto cash_put = *kiyosi::make_cash_or_nothing_option(
        kiyosi::option_type::put, 100.0, 10.0, valuation, expiry);
    const kiyosi::AnalyticDigitalEngine digital;
    const auto call_value = digital.price(cash_call, context);
    const auto put_value = digital.price(cash_put, context);
    REQUIRE(call_value.has_value());
    REQUIRE(put_value.has_value());
    CHECK(call_value->has(kiyosi::risk_measure::price));
    CHECK(call_value->has(kiyosi::risk_measure::delta));
    CHECK(call_value->has(kiyosi::risk_measure::gamma));
    CHECK_FALSE(call_value->has(kiyosi::risk_measure::vega));
    CHECK_THAT(risk_value(*call_value, kiyosi::risk_measure::price) +
                   risk_value(*put_value, kiyosi::risk_measure::price),
               WithinAbs(10.0 * std::exp(-0.04), 1e-10));
    CHECK_FALSE(kiyosi::make_cash_or_nothing_option(
                    kiyosi::option_type::call, 100.0, 0.0, valuation, expiry)
                    .has_value());
}

TEST_CASE("Barrier in and out prices compose to vanilla")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto down_out = *kiyosi::make_barrier_option({
        kiyosi::option_type::call, 100.0, valuation, expiry, 90.0, kiyosi::barrier_type::down_and_out});
    const auto down_in = *kiyosi::make_barrier_option({
        kiyosi::option_type::call, 100.0, valuation, expiry, 90.0, kiyosi::barrier_type::down_and_in});
    const auto barrier_out = kiyosi::AnalyticBarrierEngine{}.price(down_out, context);
    const auto barrier_in = kiyosi::AnalyticBarrierEngine{}.price(down_in, context);
    const auto vanilla = kiyosi::AnalyticVanillaEngine{}.price(
        *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, valuation, expiry), context);
    REQUIRE(barrier_out.has_value());
    REQUIRE(barrier_in.has_value());
    REQUIRE(vanilla.has_value());
    CHECK_THAT(risk_value(*barrier_out, kiyosi::risk_measure::price) +
                   risk_value(*barrier_in, kiyosi::risk_measure::price),
               WithinAbs(risk_value(*vanilla, kiyosi::risk_measure::price), 1e-5));
}

TEST_CASE("Scheduled barrier contracts price analytically")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto scheduled = kiyosi::make_barrier_option({
        kiyosi::option_type::call, 100.0, valuation, expiry, 90.0, kiyosi::barrier_type::down_and_out,
        0.0, kiyosi::rebate_timing::at_expiry, kiyosi::observation_mode::scheduled,
        std::vector<kiyosi::date>{valuation + std::chrono::days{30}}});
    REQUIRE(scheduled.has_value());
    CHECK(kiyosi::AnalyticBarrierEngine{}.price(*scheduled, context).has_value());
}

TEST_CASE("Deferred CPU instruments expose validated pricing paths")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = day(2025, 7, 1);
    auto parameters = kiyosi::make_bsm_parameters(0.03, 0.01, 0.2);
    auto context = kiyosi::make_pricing_context(*parameters, 100.0, valuation);
    REQUIRE(context.has_value());

    auto asian = kiyosi::make_geometric_average_option(
        kiyosi::option_type::call, 100.0, valuation, valuation, expiry);
    REQUIRE(asian.has_value());
    auto asian_result = kiyosi::GeometricAverageAsianEngine{}.price(*asian, *context);
    REQUIRE(asian_result.has_value());
    REQUIRE(asian_result->has(kiyosi::risk_measure::price));
    REQUIRE(*asian_result->get(kiyosi::risk_measure::price) > 0.0);

    auto note = kiyosi::make_binary_snowball_option({
        {0.1}, 0.05, 100.0, {110.0}, 100.0, 60.0, {expiry},
        kiyosi::barrier_touch_status::none, 1.0, valuation, expiry});
    REQUIRE(note);
    kiyosi::MonteCarloBinarySnowballEngine engine{{128, 7}};
    auto note_result = engine.price(*note, *context);
    REQUIRE(note_result.has_value());
    REQUIRE(note_result->has(kiyosi::risk_measure::price));
}

TEST_CASE("Numerical analytics expose shared risk measures")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = day(2025, 7, 1);
    auto parameters = kiyosi::make_bsm_parameters(0.03, 0.01, 0.2);
    auto context = kiyosi::make_pricing_context(*parameters, 100.0, valuation);
    REQUIRE(context.has_value());
    auto option = kiyosi::make_european_option(
        kiyosi::option_type::call, 100.0, valuation, expiry);
    REQUIRE(option.has_value());
    auto analytics = kiyosi::numerical_analytics(kiyosi::CrrVanillaEngine{64}, *option, *context);
    REQUIRE(analytics.has_value());
    CHECK(analytics->has(kiyosi::risk_measure::speed));
    CHECK(analytics->has(kiyosi::risk_measure::rho));
    kiyosi::NumericalAnalyticsEngine<kiyosi::CrrVanillaEngine> shared{kiyosi::CrrVanillaEngine{64}};
    auto shared_result = shared.price(*option, *context);
    REQUIRE(shared_result.has_value());
    CHECK(shared_result->has(kiyosi::risk_measure::vega));
}

TEST_CASE("Structured coupon replacement preserves the original note")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = day(2025, 7, 1);
    const auto note = kiyosi::make_snowball_option({
        {0.1}, 0.05, 100.0, 60.0, {110.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::at_expiry, kiyosi::barrier_touch_status::none,
        1.0, valuation, expiry});
    REQUIRE(note);
    const auto replaced = note->with_coupon_rate(0.08);
    REQUIRE(replaced);
    CHECK(note->maturity_coupon_rate() == 0.05);
    CHECK(replaced->maturity_coupon_rate() == 0.08);
}

} // namespace
