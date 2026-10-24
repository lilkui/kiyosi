#include <catch2/catch_test_macros.hpp>

#include <chrono>
#include <cmath>
#include <limits>
#include <utility>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using namespace kiyosi;
constexpr auto start = test::day(2025, 1, 1);
constexpr auto expiry = test::day(2026, 1, 1);

DoubleBarrierOptionTerms terms()
{
    return {.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = expiry, .lower_barrier = 80.0, .upper_barrier = 120.0, .barrier_type = DoubleBarrierType::knock_out};
}
} // namespace

TEST_CASE("Double barrier terms validate contract boundaries", "[double-barrier]")
{
    const auto original = terms();
    const auto check = [](const auto& result, ErrorCategory category) {
        REQUIRE_FALSE(result);
        CHECK(result.error().category == category);
    };
    for (const double level : {0.0, -1.0, std::numeric_limits<double>::infinity(),
                               std::numeric_limits<double>::quiet_NaN()}) {
        auto invalid = original;
        invalid.lower_barrier = level;
        check(make_double_barrier_option(invalid), ErrorCategory::invalid_parameter);
        invalid = original;
        invalid.upper_barrier = level;
        check(make_double_barrier_option(invalid), ErrorCategory::invalid_parameter);
    }
    for (const double upper : {79.0, 80.0}) {
        auto invalid = original;
        invalid.upper_barrier = upper;
        check(make_double_barrier_option(invalid), ErrorCategory::invalid_parameter);
    }
    auto invalid = original;
    invalid.strike = 0.0;
    check(make_double_barrier_option(invalid), ErrorCategory::invalid_strike);
    invalid = original;
    invalid.option_type = static_cast<OptionType>(99);
    check(make_double_barrier_option(invalid), ErrorCategory::invalid_option);
    invalid = original;
    invalid.barrier_type = static_cast<DoubleBarrierType>(99);
    check(make_double_barrier_option(invalid), ErrorCategory::invalid_option);
    for (const double rebate : {-1.0, std::numeric_limits<double>::infinity(),
                                std::numeric_limits<double>::quiet_NaN()}) {
        invalid = original;
        invalid.rebate = rebate;
        check(make_double_barrier_option(invalid), ErrorCategory::invalid_parameter);
    }
    invalid = original;
    invalid.rebate_timing = static_cast<RebateTiming>(99);
    check(make_double_barrier_option(invalid), ErrorCategory::invalid_option);
    invalid = original;
    invalid.barrier_type = DoubleBarrierType::knock_in;
    invalid.rebate_timing = RebateTiming::at_hit;
    check(make_double_barrier_option(invalid), ErrorCategory::invalid_option);
    invalid = original;
    invalid.touch_state = static_cast<BarrierTouchState>(99);
    check(make_double_barrier_option(invalid), ErrorCategory::invalid_option);

    for (const double strike : {60.0, 140.0}) {
        auto valid = original;
        valid.strike = strike;
        const auto option = make_double_barrier_option(valid);
        REQUIRE(option);
        CHECK(option->strike() == strike);
    }
}

TEST_CASE("Double barrier schedules reject incompatible monitoring terms", "[double-barrier]")
{
    const auto reject = [](DoubleBarrierOptionTerms input) {
        const auto result = make_double_barrier_option(std::move(input));
        REQUIRE_FALSE(result);
        CHECK(result.error().category == ErrorCategory::invalid_schedule);
    };
    auto invalid = terms();
    invalid.observation_dates = {expiry};
    reject(invalid);
    invalid.observation_dates.clear();
    invalid.observation_mode = ObservationMode::scheduled;
    reject(invalid);
    invalid.observation_mode = static_cast<ObservationMode>(99);
    reject(invalid);
    for (const auto& dates : {std::vector<Date>{expiry, start}, std::vector<Date>{start, start},
                              std::vector<Date>{start - std::chrono::days{1}}}) {
        invalid = terms();
        invalid.observation_mode = ObservationMode::scheduled;
        invalid.observation_dates = dates;
        const auto result = make_double_barrier_option(invalid);
        REQUIRE_FALSE(result);
    }
    auto valid = terms();
    valid.observation_mode = ObservationMode::scheduled;
    valid.observation_dates = {start + std::chrono::days{180}};
    const auto option = make_double_barrier_option(valid);
    REQUIRE(option);
    CHECK(option->observation_dates() == valid.observation_dates);
}

TEST_CASE("Double barrier defaults and inclusive touches are core defined", "[double-barrier]")
{
    const auto option = make_double_barrier_option(terms());
    REQUIRE(option);
    CHECK(option->rebate() == 0.0);
    CHECK(option->rebate_timing() == RebateTiming::at_expiry);
    CHECK(option->observation_mode() == ObservationMode::continuous);
    CHECK_FALSE(option->touch_state());
    CHECK(option->observation_dates().empty());
    const auto& barrier = option->barrier_terms();
    for (const double spot : {79.0, 80.0, 120.0, 121.0})
        CHECK(barrier.is_breached_by(spot));
    for (const double spot : {std::nextafter(80.0, 120.0), 100.0, std::nextafter(120.0, 80.0)})
        CHECK_FALSE(barrier.is_breached_by(spot));
}

TEST_CASE("Double barrier history replacement preserves contract terms", "[double-barrier]")
{
    auto input = terms();
    input.rebate = 3.0;
    input.observation_mode = ObservationMode::scheduled;
    input.observation_dates = {start, start + std::chrono::days{180}};
    const auto original = make_double_barrier_option(input);
    REQUIRE(original);
    const auto replaced = original->replace(BarrierTouchState::touched);
    REQUIRE(replaced);
    CHECK_FALSE(original->touch_state());
    CHECK(replaced->touch_state() == BarrierTouchState::touched);
    CHECK(replaced->lower_barrier() == 80.0);
    CHECK(replaced->upper_barrier() == 120.0);
    CHECK(replaced->rebate() == 3.0);
    CHECK(replaced->barrier_type() == DoubleBarrierType::knock_out);
    CHECK(replaced->observation_dates() == input.observation_dates);
    const auto reset = replaced->replace(std::nullopt);
    REQUIRE(reset);
    CHECK(*reset == *original);
    const auto invalid = original->replace(static_cast<BarrierTouchState>(99));
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == ErrorCategory::invalid_option);
    CHECK(*original == *make_double_barrier_option(input));

    const auto missing = original->barrier_terms().was_touched_before(start_of_day(start) + std::chrono::seconds{1});
    REQUIRE_FALSE(missing);
    CHECK(missing.error().category == ErrorCategory::invalid_parameter);
    const auto impossible = replaced->barrier_terms().was_touched_before(start_of_day(start));
    REQUIRE_FALSE(impossible);
    CHECK(impossible.error().category == ErrorCategory::invalid_option);
}
