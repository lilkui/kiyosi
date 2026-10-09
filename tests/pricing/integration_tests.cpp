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

TEST_CASE("Bjerksund rejects negative transformed rates before expiry")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const kiyosi::BjerksundStenslandVanillaEngine engine;
    for (const auto type : {kiyosi::OptionType::call, kiyosi::OptionType::put}) {
        const bool call = type == kiyosi::OptionType::call;
        const auto option = *kiyosi::make_american_option(type, 100.0, start, end);
        const auto parameters = *kiyosi::make_bsm_parameters(call ? -0.01 : 0.0, call ? 0.0 : -0.01, 0.2);
        const double spot = call ? 200.0 : 50.0;
        const auto context = *kiyosi::make_pricing_context(parameters, spot, start);
        const auto price = engine.price(option, context);
        REQUIRE_FALSE(price);
        CHECK(price.error().category == kiyosi::ErrorCategory::unsupported_operation);
        const auto greeks = engine.price_with_greeks(option, context, {kiyosi::Greek::delta});
        REQUIRE_FALSE(greeks);
        CHECK(greeks.error().category == kiyosi::ErrorCategory::unsupported_operation);
        const auto expired = engine.price(option, *kiyosi::make_pricing_context(parameters, spot, end));
        REQUIRE(expired);
        CHECK(*expired == (call ? 100.0 : 50.0));
        const auto zero_rate = engine.price(option, *kiyosi::make_pricing_context(
                                                        *kiyosi::make_bsm_parameters(0.0, 0.0, 0.2), spot, start));
        REQUIRE(zero_rate);
        CHECK(*zero_rate >= (call ? 100.0 : 50.0));
    }
}

TEST_CASE("Quadrature includes high variance payoff tails")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2035, 1, 1);
    for (const double volatility : {3.0, 5.0}) {
        const auto context = *kiyosi::make_pricing_context(
            *kiyosi::make_bsm_parameters(0.05, 0.02, volatility), 100.0, start);
        for (const auto type : {kiyosi::OptionType::call, kiyosi::OptionType::put}) {
            const auto vanilla = *kiyosi::make_european_option(type, 100.0, start, end);
            const auto asset = *kiyosi::make_asset_or_nothing_option(type, 100.0, start, end);
            const auto cash = *kiyosi::make_cash_or_nothing_option(type, 100.0, 10.0, start, end);
            for (const auto& pair : {
                     std::pair{kiyosi::QuadratureVanillaEngine{}.price(vanilla, context),
                               kiyosi::AnalyticVanillaEngine{}.price(vanilla, context)},
                     std::pair{kiyosi::QuadratureDigitalEngine{}.price(asset, context),
                               kiyosi::AnalyticDigitalEngine{}.price(asset, context)},
                     std::pair{kiyosi::QuadratureDigitalEngine{}.price(cash, context),
                               kiyosi::AnalyticDigitalEngine{}.price(cash, context)}}) {
                REQUIRE(pair.first);
                REQUIRE(pair.second);
                CHECK_THAT(*pair.first, Catch::Matchers::WithinAbs(*pair.second, 1e-8));
            }
        }
    }
}

TEST_CASE("Trading Monte Carlo rejects invalid simulation paths")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2025, 1, 2);
    const auto accumulator = *kiyosi::make_accumulator(
        {.strike = 100.0, .knock_out_level = 120.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = end});
    const auto note = *kiyosi::make_binary_snowball_option(
        {.knock_out_coupon_rates = {0.1}, .maturity_coupon_rate = 0.1, .knock_out_levels = {120.0}, .observation_dates = {end}, .effective_date = start, .expiry_date = end});
    for (const auto [rate, volatility] : {
             std::pair{0.05, 1e308}, std::pair{0.05, 1000.0}, std::pair{1e308, 0.2}}) {
        const auto context = *kiyosi::make_pricing_context(
            *kiyosi::make_bsm_parameters(rate, 0.02, volatility), 100.0, start);
        for (const auto& price : {
                 kiyosi::MonteCarloAccumulatorEngine{2, 1}.price(accumulator, context),
                 kiyosi::MonteCarloBinarySnowballEngine{2, 1}.price(note, context)}) {
            REQUIRE_FALSE(price);
            CHECK(price.error().category == kiyosi::ErrorCategory::invalid_result);
        }
    }
}

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
    const auto call_value = digital.price_with_greeks(cash_call, context, kiyosi::GreeksRequest{kiyosi::Greek::delta, kiyosi::Greek::gamma});
    const auto put_value = digital.price(cash_put, context);
    REQUIRE(call_value.has_value());
    REQUIRE(put_value.has_value());
    CHECK(std::isfinite(call_value->price()));
    CHECK(call_value->has(kiyosi::Greek::delta));
    CHECK(call_value->has(kiyosi::Greek::gamma));
    CHECK_FALSE(call_value->has(kiyosi::Greek::vega));
    CHECK_THAT(call_value->price() +
                   *put_value,
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
    CHECK_THAT(*barrier_out +
                   *barrier_in,
               WithinAbs(*vanilla, 1e-5));
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
                                                        .observation_dates = {valuation + std::chrono::days{30}, expiry_date}});
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
    auto asian_result = kiyosi::AnalyticGeometricAveragePriceEngine{}.price(*asian, *context);
    REQUIRE(asian_result.has_value());
    REQUIRE(*asian_result > 0.0);

    auto note = kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.1},
                                                     .maturity_coupon_rate = 0.05,
                                                     .knock_out_levels = {110.0},
                                                     .observation_dates = {expiry_date},
                                                     .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                     .principal_ratio = 1.0,
                                                     .effective_date = valuation,
                                                     .expiry_date = expiry_date});
    REQUIRE(note);
    const kiyosi::MonteCarloBinarySnowballEngine engine{{128, 7}};
    auto note_result = engine.price(*note, *context);
    REQUIRE(note_result.has_value());
    CHECK(*note_result > 0.0);
}

TEST_CASE("Numerical analytics expose shared Greeks")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = day(2025, 7, 1);
    auto parameters = kiyosi::make_bsm_parameters(0.03, 0.01, 0.2);
    auto context = kiyosi::make_pricing_context(*parameters, 100.0, valuation);
    REQUIRE(context.has_value());
    auto option = kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    REQUIRE(option.has_value());
    auto analytics = kiyosi::calculate_numerical_greeks(kiyosi::CoxRossRubinsteinVanillaEngine{64}, *option, *context);
    REQUIRE(analytics.has_value());
    CHECK(analytics->has(kiyosi::Greek::speed));
    CHECK(analytics->has(kiyosi::Greek::rho));
    CHECK(analytics->has(kiyosi::Greek::vega));
}

} // namespace
