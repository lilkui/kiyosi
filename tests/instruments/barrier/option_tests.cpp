#include <catch2/catch_test_macros.hpp>

#include <chrono>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::test::day;
}

TEST_CASE("Barrier option factory rejects invalid contracts")
{
    const auto effective_date = day(2025, 1, 6);
    const auto expiry_date = effective_date + std::chrono::days{365};
    const auto terms = kiyosi::BarrierOptionTerms{.option_type = kiyosi::OptionType::call,
                                                  .strike = 100.0,
                                                  .effective_date = effective_date,
                                                  .expiry_date = expiry_date,
                                                  .barrier_level = 90.0,
                                                  .barrier_type = kiyosi::BarrierType::down_and_in};
    auto invalid = terms;
    invalid.option_type = static_cast<kiyosi::OptionType>(99);
    const auto invalid_type = kiyosi::make_barrier_option(invalid);
    REQUIRE_FALSE(invalid_type.has_value());
    CHECK(invalid_type.error().category == kiyosi::ErrorCategory::invalid_option);
    invalid = terms;
    invalid.strike = -1.0;
    const auto invalid_strike = kiyosi::make_barrier_option(invalid);
    REQUIRE_FALSE(invalid_strike.has_value());
    CHECK(invalid_strike.error().category == kiyosi::ErrorCategory::invalid_strike);
    invalid = terms;
    invalid.barrier_level = -1.0;
    const auto invalid_level = kiyosi::make_barrier_option(invalid);
    REQUIRE_FALSE(invalid_level.has_value());
    CHECK(invalid_level.error().category == kiyosi::ErrorCategory::invalid_parameter);
    invalid = terms;
    invalid.rebate = -1.0;
    const auto invalid_rebate = kiyosi::make_barrier_option(invalid);
    REQUIRE_FALSE(invalid_rebate.has_value());
    CHECK(invalid_rebate.error().category == kiyosi::ErrorCategory::invalid_parameter);
    invalid = terms;
    invalid.rebate = 10.0;
    invalid.rebate_timing = kiyosi::RebateTiming::at_hit;
    const auto invalid_timing = kiyosi::make_barrier_option(invalid);
    REQUIRE_FALSE(invalid_timing.has_value());
    CHECK(invalid_timing.error().category == kiyosi::ErrorCategory::invalid_option);
    invalid = terms;
    invalid.touch_state = static_cast<kiyosi::BarrierTouchState>(99);
    const auto invalid_state = kiyosi::make_barrier_option(invalid);
    REQUIRE_FALSE(invalid_state.has_value());
    CHECK(invalid_state.error().category == kiyosi::ErrorCategory::invalid_option);
}

TEST_CASE("Barrier option exposes its contractual terms")
{
    const auto effective_date = day(2025, 1, 6);
    const auto expiry_date = effective_date + std::chrono::days{365};
    const auto option = kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                     .strike = 100.0,
                                                     .effective_date = effective_date,
                                                     .expiry_date = expiry_date,
                                                     .barrier_level = 120.0,
                                                     .barrier_type = kiyosi::BarrierType::up_and_out,
                                                     .rebate = 5.0,
                                                     .rebate_timing = kiyosi::RebateTiming::at_expiry});
    REQUIRE(option.has_value());
    CHECK(option->option_type() == kiyosi::OptionType::call);
    CHECK(option->strike() == 100.0);
    CHECK(option->rebate() == 5.0);
    CHECK(option->rebate_timing() == kiyosi::RebateTiming::at_expiry);
    CHECK(option->barrier_level() == 120.0);
    CHECK(option->barrier_type() == kiyosi::BarrierType::up_and_out);
    CHECK(option->effective_date() == effective_date);
    CHECK(option->expiry_date() == expiry_date);
}
