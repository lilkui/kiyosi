#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <chrono>
#include <limits>
#include <numbers>
#include <string>
#include <type_traits>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"

namespace {

using kiyosi::test::day;

TEST_CASE("Analytic barriers settle at expiry for every touch and knock state")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2), 100.0, expiry);
    for (const auto type : {kiyosi::BarrierType::up_and_in, kiyosi::BarrierType::up_and_out}) {
        for (const auto state : {kiyosi::BarrierTouchState::untouched, kiyosi::BarrierTouchState::touched}) {
            for (const double level : {90.0, 120.0}) {
                for (const auto timing : {kiyosi::RebateTiming::at_hit, kiyosi::RebateTiming::at_expiry}) {
                    if (type == kiyosi::BarrierType::up_and_in && timing == kiyosi::RebateTiming::at_hit)
                        continue;
                    CAPTURE(type, state, level, timing);
                    const auto option = kiyosi::make_barrier_option(
                        {.option_type = kiyosi::OptionType::call,
                         .strike = 80.0,
                         .effective_date = effective,
                         .expiry_date = expiry,
                         .barrier_level = level,
                         .barrier_type = type,
                         .rebate = 7.0,
                         .rebate_timing = timing,
                         .touch_state = state});
                    REQUIRE(option);
                    const bool prior_touch = state == kiyosi::BarrierTouchState::touched;
                    const bool touched = prior_touch || level <= 100.0;
                    const double unpaid_rebate = prior_touch && timing == kiyosi::RebateTiming::at_hit ? 0.0 : 7.0;
                    const double expected = type == kiyosi::BarrierType::up_and_in
                                                ? (touched ? 20.0 : 7.0)
                                                : (touched ? unpaid_rebate : 20.0);
                    const auto result = kiyosi::AnalyticBarrierEngine{}.price(*option, context);
                    REQUIRE(result);
                    CHECK(*result == expected);
                }
            }
        }
    }
}

TEST_CASE("Already-hit barrier rebates respect expiry_date payment timing")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto barrier = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                       .strike = 100.0,
                                                       .effective_date = valuation,
                                                       .expiry_date = expiry_date,
                                                       .barrier_level = 90.0,
                                                       .barrier_type = kiyosi::BarrierType::up_and_out,
                                                       .rebate = 10.0,
                                                       .rebate_timing = kiyosi::RebateTiming::at_expiry});

    const auto result = kiyosi::AnalyticBarrierEngine{}.price(barrier, context);
    REQUIRE(result.has_value());
    CHECK_THAT(*result, Catch::Matchers::WithinAbs(10.0 * std::exp(-0.05), 1e-12));
}

TEST_CASE("Barrier hit rebates use the finite first-hit payment decomposition")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.03, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto barrier = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                       .strike = 1'000'000'000.0,
                                                       .effective_date = valuation,
                                                       .expiry_date = expiry_date,
                                                       .barrier_level = 110.0,
                                                       .barrier_type = kiyosi::BarrierType::up_and_out,
                                                       .rebate = 10.0,
                                                       .rebate_timing = kiyosi::RebateTiming::at_hit});

    const auto result = kiyosi::AnalyticBarrierEngine{}.price(barrier, context);
    REQUIRE(result.has_value());
    const double distance = std::log(1.1);
    const double root = std::sqrt(2.0 * 0.05 * 0.2 * 0.2);
    const auto normal_cdf = [](double value) { return 0.5 * std::erfc(-value / std::numbers::sqrt2); };
    const double discounted_hit = std::exp(-root * distance / (0.2 * 0.2)) *
                                      normal_cdf((root - distance) / 0.2) +
                                  std::exp(root * distance / (0.2 * 0.2)) *
                                      normal_cdf((-root - distance) / 0.2);
    CHECK_THAT(*result, Catch::Matchers::WithinAbs(10.0 * discounted_hit, 1e-6));
}

TEST_CASE("Barrier hit rebates reject an unstable negative-rate limit")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(-0.02, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto barrier = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                       .strike = 1'000'000'000.0,
                                                       .effective_date = valuation,
                                                       .expiry_date = expiry_date,
                                                       .barrier_level = 110.0,
                                                       .barrier_type = kiyosi::BarrierType::up_and_out,
                                                       .rebate = 10.0,
                                                       .rebate_timing = kiyosi::RebateTiming::at_hit});

    const auto result = kiyosi::AnalyticBarrierEngine{}.price(barrier, context);
    REQUIRE_FALSE(result.has_value());
    CHECK(result.error().category == kiyosi::ErrorCategory::invalid_result);
}

TEST_CASE("Barrier engines price prior touches from history instead of current spot")
{
    const auto effective = day(2025, 1, 1);
    const auto valuation = day(2025, 7, 1);
    const auto expiry = day(2026, 1, 1);
    const auto parameters = kiyosi::make_bsm_parameters(0.04, 0.01, 0.2);
    REQUIRE(parameters);
    const auto context_checked = kiyosi::make_pricing_context(
        *parameters, 100.0, valuation);
    REQUIRE(context_checked);
    const auto context = *context_checked;
    auto terms = kiyosi::BarrierOptionTerms{.option_type = kiyosi::OptionType::call,
                                            .strike = 100.0,
                                            .effective_date = effective,
                                            .expiry_date = expiry,
                                            .barrier_level = 120.0,
                                            .barrier_type = kiyosi::BarrierType::up_and_out};
    const auto missing_checked = kiyosi::make_barrier_option(terms);
    REQUIRE(missing_checked);
    const auto missing = *missing_checked;
    CHECK_FALSE(missing.touch_state());
    const auto missing_result = kiyosi::AnalyticBarrierEngine{}.price(missing, context);
    REQUIRE_FALSE(missing_result);
    CHECK(missing_result.error().category == kiyosi::ErrorCategory::invalid_parameter);
    const auto missing_fd = kiyosi::FiniteDifferenceBarrierEngine{}.price(missing, context);
    REQUIRE_FALSE(missing_fd.has_value());
    CHECK(missing_fd.error().category ==
          kiyosi::ErrorCategory::invalid_parameter);

    terms.touch_state = kiyosi::BarrierTouchState::untouched;
    const auto untouched_checked = kiyosi::make_barrier_option(terms);
    REQUIRE(untouched_checked);
    const auto untouched = *untouched_checked;
    terms.touch_state = kiyosi::BarrierTouchState::touched;
    const auto touched_checked = kiyosi::make_barrier_option(terms);
    REQUIRE(touched_checked);
    const auto touched = *touched_checked;
    CHECK(*kiyosi::AnalyticBarrierEngine{}.price(untouched, context) > 0.0);
    CHECK(*kiyosi::AnalyticBarrierEngine{}.price(touched, context) == 0.0);
    CHECK(*kiyosi::FiniteDifferenceBarrierEngine{}.price(touched, context) == 0.0);

    terms.barrier_type = kiyosi::BarrierType::up_and_in;
    const auto knocked_in_checked = kiyosi::make_barrier_option(terms);
    REQUIRE(knocked_in_checked);
    const auto knocked_in = *knocked_in_checked;
    const auto vanilla_checked = kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective, expiry);
    REQUIRE(vanilla_checked);
    const auto vanilla = *vanilla_checked;
    CHECK_THAT(*kiyosi::AnalyticBarrierEngine{}.price(knocked_in, context),
               Catch::Matchers::WithinAbs(*kiyosi::AnalyticVanillaEngine{}.price(vanilla, context), 1e-12));

    terms.barrier_type = kiyosi::BarrierType::up_and_out;
    terms.rebate = 10.0;
    terms.rebate_timing = kiyosi::RebateTiming::at_hit;
    CHECK(*kiyosi::AnalyticBarrierEngine{}.price(*kiyosi::make_barrier_option(terms), context) == 0.0);
    terms.rebate_timing = kiyosi::RebateTiming::at_expiry;
    CHECK_THAT(*kiyosi::AnalyticBarrierEngine{}.price(*kiyosi::make_barrier_option(terms), context),
               Catch::Matchers::WithinAbs(10.0 * std::exp(-0.04 * 184.0 / 365.0), 1e-12));
}

TEST_CASE("Analytic scheduled barriers stop monitoring after their final observation")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2), 100.0, day(2025, 1, 3));
    const auto vanilla = *kiyosi::AnalyticVanillaEngine{}.price(
        *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective, expiry), context);
    auto terms = kiyosi::BarrierOptionTerms{.option_type = kiyosi::OptionType::call,
                                            .strike = 100.0,
                                            .effective_date = effective,
                                            .expiry_date = expiry,
                                            .barrier_level = 120.0,
                                            .barrier_type = kiyosi::BarrierType::up_and_out,
                                            .observation_mode = kiyosi::ObservationMode::scheduled,
                                            .observation_dates = {day(2025, 1, 2)},
                                            .touch_state = kiyosi::BarrierTouchState::untouched};
    CHECK_THAT(*kiyosi::AnalyticBarrierEngine{}.price(*kiyosi::make_barrier_option(terms), context),
               Catch::Matchers::WithinAbs(vanilla, 1e-12));
    terms.barrier_type = kiyosi::BarrierType::up_and_in;
    terms.rebate = 10.0;
    CHECK_THAT(*kiyosi::AnalyticBarrierEngine{}.price(*kiyosi::make_barrier_option(terms), context),
               Catch::Matchers::WithinAbs(10.0 * std::exp(-0.05 * 363.0 / 365.0), 1e-12));
}

TEST_CASE("Scheduled barriers settle exactly at and after their final fixing", "[audit-fixes]")
{
    const auto effective = day(2025, 1, 1);
    const auto fixing = day(2025, 1, 2);
    const auto expiry = day(2026, 1, 1);
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto check = [&](const auto& engine) {
        for (const auto valuation : {effective, fixing, day(2025, 1, 3)}) {
            for (const auto kind : {kiyosi::BarrierType::up_and_in, kiyosi::BarrierType::up_and_out}) {
                for (const double rebate : {0.0, 10.0}) {
                    CAPTURE(valuation, kind, rebate);
                    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
                    const auto option = *kiyosi::make_barrier_option(
                        {.option_type = kiyosi::OptionType::call, .strike = 100.0, .effective_date = effective, .expiry_date = expiry,
                         .barrier_level = 120.0, .barrier_type = kind, .rebate = rebate,
                         .observation_mode = kiyosi::ObservationMode::scheduled, .observation_dates = {fixing},
                         .touch_state = kiyosi::BarrierTouchState::untouched});
                    const auto price = engine.price(option, context);
                    REQUIRE(price);
                    if (valuation == effective) {
                        CHECK(*price > 0.0);
                        continue;
                    }
                    const double expected = kind == kiyosi::BarrierType::up_and_in
                                                ? rebate * std::exp(-0.05 * *kiyosi::year_fraction(valuation, expiry))
                                                : *kiyosi::AnalyticVanillaEngine{}.price(
                                                      *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective, expiry), context);
                    CHECK(*price == expected);
                }
            }
        }
    };
    check(kiyosi::AnalyticBarrierEngine{});
    check(kiyosi::FiniteDifferenceBarrierEngine{});
}

TEST_CASE("Scheduled barrier fixing does not recur later on its observation date")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto fixing = day(2025, 1, 2);
    const auto time = kiyosi::start_of_day(fixing) + std::chrono::hours{12};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2), 125.0, time);
    const auto barrier = *kiyosi::make_barrier_option(
        {.option_type = kiyosi::OptionType::call, .strike = 100.0, .effective_date = effective, .expiry_date = expiry, .barrier_level = 120.0, .barrier_type = kiyosi::BarrierType::up_and_out, .observation_mode = kiyosi::ObservationMode::scheduled, .observation_dates = {fixing}, .touch_state = kiyosi::BarrierTouchState::untouched});
    const auto vanilla = *kiyosi::AnalyticVanillaEngine{}.price(
        *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective, expiry), context);
    CHECK_THAT(*kiyosi::AnalyticBarrierEngine{}.price(barrier, context),
               Catch::Matchers::WithinAbs(vanilla, 1e-12));
    CHECK_THAT(*kiyosi::FiniteDifferenceBarrierEngine{}.price(barrier, context),
               Catch::Matchers::WithinAbs(vanilla, 0.1));
}

} // namespace
