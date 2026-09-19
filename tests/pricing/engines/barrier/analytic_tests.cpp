#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <chrono>
#include <limits>
#include <string>
#include <type_traits>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"

namespace {

using kiyosi::test::day;
using kiyosi::test::risk_value;

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
    CHECK_THAT(risk_value(*result, kiyosi::RiskMeasure::price), Catch::Matchers::WithinAbs(10.0 * std::exp(-0.05), 1e-12));
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
    const auto normal_cdf = [](double value) { return 0.5 * std::erfc(-value / std::sqrt(2.0)); };
    const double discounted_hit = std::exp(-root * distance / (0.2 * 0.2)) *
                                      normal_cdf((root - distance) / 0.2) +
                                  std::exp(root * distance / (0.2 * 0.2)) *
                                      normal_cdf((-root - distance) / 0.2);
    CHECK_THAT(risk_value(*result, kiyosi::RiskMeasure::price), Catch::Matchers::WithinAbs(10.0 * discounted_hit, 1e-6));
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

}
