#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <chrono>
#include <cmath>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {

using kiyosi::test::day;

}

TEST_CASE("Binary barriers expose observation intervals")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto observation_dates = std::vector<kiyosi::Date>{valuation + std::chrono::days{30},
                                                             valuation + std::chrono::days{180}};
    const auto binary = kiyosi::make_cash_binary_barrier_option(
        {.option_type = kiyosi::OptionType::call,
         .strike = 100.0,
         .effective_date = valuation,
         .expiry_date = expiry_date,
         .barrier_level = 90.0,
         .barrier_type = kiyosi::BarrierType::down_and_in,
         .observation_mode = kiyosi::ObservationMode::scheduled,
         .observation_dates = observation_dates},
        10.0);
    REQUIRE(binary);
    CHECK_THAT(binary->mean_observation_year_fraction(),
               Catch::Matchers::WithinAbs(180.0 / 365.0 / 2.0, 1e-12));
}

TEST_CASE("Touch factories require only payoff-relevant terms")
{
    const auto effective_date = day(2025, 1, 6);
    const auto expiry_date = effective_date + std::chrono::days{365};
    const auto observation_dates = std::vector<kiyosi::Date>{effective_date + std::chrono::days{30}, expiry_date};
    const auto cash = kiyosi::make_cash_one_touch_up(
        effective_date, expiry_date, 130.0, 10.0, kiyosi::SettlementTiming::at_hit,
        kiyosi::ObservationMode::scheduled, observation_dates);
    const auto asset = kiyosi::make_asset_no_touch_down(effective_date, expiry_date, 70.0);
    REQUIRE(cash);
    REQUIRE(asset);
    CHECK(cash->is_one_touch());
    CHECK(cash->is_up());
    CHECK(cash->payoff_type() == kiyosi::PayoffType::cash);
    CHECK(cash->settlement_timing() == kiyosi::SettlementTiming::at_hit);
    CHECK(cash->observation_mode() == kiyosi::ObservationMode::scheduled);
    CHECK(cash->observation_dates() == observation_dates);
    CHECK_FALSE(asset->is_one_touch());
    CHECK_FALSE(asset->is_up());
    CHECK(asset->payoff_type() == kiyosi::PayoffType::asset);
    CHECK(asset->settlement_timing() == kiyosi::SettlementTiming::at_expiry);
}

TEST_CASE("Binary and touch contracts distinguish past settlement from future payoff")
{
    const auto effective = day(2025, 1, 1);
    const auto valuation = day(2025, 7, 1);
    const auto expiry = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, valuation);
    const kiyosi::AnalyticBinaryBarrierEngine engine;
    auto terms = kiyosi::BinaryBarrierTerms{.option_type = kiyosi::OptionType::call,
                                            .strike = 90.0,
                                            .effective_date = effective,
                                            .expiry_date = expiry,
                                            .barrier_level = 120.0,
                                            .barrier_type = kiyosi::BarrierType::up_and_out};
    const auto missing = engine.price(*kiyosi::make_cash_binary_barrier_option(terms, 10.0), context);
    REQUIRE_FALSE(missing);
    CHECK(missing.error().category == kiyosi::ErrorCategory::invalid_parameter);
    terms.touch_state = kiyosi::BarrierTouchState::untouched;
    CHECK(*engine.price(*kiyosi::make_cash_binary_barrier_option(terms, 10.0), context) > 0.0);
    terms.touch_state = kiyosi::BarrierTouchState::touched;
    CHECK(*engine.price(*kiyosi::make_cash_binary_barrier_option(terms, 10.0), context) == 0.0);
    terms.barrier_type = kiyosi::BarrierType::up_and_in;
    CHECK(*engine.price(*kiyosi::make_cash_binary_barrier_option(terms, 10.0), context) > 0.0);

    const auto at_hit = *kiyosi::make_cash_one_touch_up(
        effective, expiry, 120.0, 10.0, kiyosi::SettlementTiming::at_hit,
        kiyosi::ObservationMode::continuous, {}, kiyosi::BarrierTouchState::touched);
    const auto at_expiry = *kiyosi::make_cash_one_touch_up(
        effective, expiry, 120.0, 10.0, kiyosi::SettlementTiming::at_expiry,
        kiyosi::ObservationMode::continuous, {}, kiyosi::BarrierTouchState::touched);
    CHECK(*engine.price(at_hit, context) == 0.0);
    CHECK_THAT(*engine.price(at_expiry, context),
               Catch::Matchers::WithinAbs(10.0 * std::exp(-0.04 * 184.0 / 365.0), 1e-12));
    const auto touching_now = *kiyosi::make_cash_one_touch_up(
        valuation, expiry, 90.0, 10.0, kiyosi::SettlementTiming::at_hit);
    CHECK(*engine.price(touching_now, context) == 10.0);
}

TEST_CASE("Scheduled binary and touch contracts stop monitoring after their final observation")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2), 100.0, day(2025, 1, 3));
    const kiyosi::AnalyticBinaryBarrierEngine engine;
    auto terms = kiyosi::BinaryBarrierTerms{.option_type = kiyosi::OptionType::call,
                                            .strike = 100.0,
                                            .effective_date = effective,
                                            .expiry_date = expiry,
                                            .barrier_level = 120.0,
                                            .barrier_type = kiyosi::BarrierType::up_and_in,
                                            .observation_mode = kiyosi::ObservationMode::scheduled,
                                            .observation_dates = {day(2025, 1, 2)},
                                            .touch_state = kiyosi::BarrierTouchState::untouched};
    CHECK(*engine.price(*kiyosi::make_cash_binary_barrier_option(terms, 10.0), context) == 0.0);
    terms.barrier_type = kiyosi::BarrierType::up_and_out;
    CHECK(*engine.price(*kiyosi::make_cash_binary_barrier_option(terms, 10.0), context) > 0.0);
    const auto one_touch = *kiyosi::make_cash_one_touch_up(
        effective, expiry, 120.0, 10.0, kiyosi::SettlementTiming::at_expiry,
        kiyosi::ObservationMode::scheduled, {day(2025, 1, 2)}, kiyosi::BarrierTouchState::untouched);
    const auto no_touch = *kiyosi::make_cash_no_touch_up(
        effective, expiry, 120.0, 10.0,
        kiyosi::ObservationMode::scheduled, {day(2025, 1, 2)}, kiyosi::BarrierTouchState::untouched);
    CHECK(*engine.price(one_touch, context) == 0.0);
    CHECK_THAT(*engine.price(no_touch, context),
               Catch::Matchers::WithinAbs(10.0 * std::exp(-0.05 * 363.0 / 365.0), 1e-12));
}
