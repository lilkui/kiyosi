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

TEST_CASE("Dates, calendars, and observation schedules are value-safe")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{10};
    REQUIRE(expiry > valuation);
    REQUIRE(kiyosi::all_days_calendar().is_trading_day(expiry));
    REQUIRE_FALSE(kiyosi::exchange_calendar().is_trading_day(day(2025, 1, 4)));

    auto custom = kiyosi::make_trading_calendar(
        [](kiyosi::date value) { return value == day(2025, 1, 2) || value == day(2025, 1, 3); }, 2);
    REQUIRE(custom.has_value());
    const auto copied_calendar = *custom;
    REQUIRE(copied_calendar.is_trading_day(day(2025, 1, 2)));
    REQUIRE(copied_calendar.annual_trading_days() == 2);

    const std::vector<kiyosi::date> observations{day(2025, 1, 2), day(2025, 1, 3)};
    REQUIRE(kiyosi::validate_schedule(observations, valuation, expiry, copied_calendar).has_value());
    REQUIRE_FALSE(kiyosi::validate_schedule(
                      std::vector<kiyosi::date>{day(2025, 1, 4)}, valuation, expiry, copied_calendar)
                      .has_value());
    REQUIRE_FALSE(kiyosi::validate_schedule(
                      std::vector<kiyosi::date>{day(2025, 1, 2), day(2025, 1, 2)}, valuation, expiry, copied_calendar)
                      .has_value());

    auto parameters = kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    auto context = kiyosi::make_pricing_context(*parameters, *kiyosi::make_asset_price(100.0), valuation, copied_calendar);
    REQUIRE(context.has_value());
    const auto context_copy = *context;
    REQUIRE(context_copy.calendar().is_trading_day(day(2025, 1, 2)));
}

TEST_CASE("Time and schedules share explicit day-count and calendar rules")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2025, 1, 6);
    const auto fraction = kiyosi::year_fraction(start, end);
    REQUIRE(fraction.has_value());
    CHECK_THAT(*fraction, Catch::Matchers::WithinAbs(5.0 / 365.0, 1e-15));

    const auto noon = kiyosi::start_of_day(start) + std::chrono::hours{12};
    const auto context = kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.01, 0.0, 0.2), *kiyosi::make_asset_price(100.0), noon);
    REQUIRE(context.has_value());
    CHECK(context->valuation_date() == start);
    CHECK(context->valuation_time().time_since_epoch() == noon.time_since_epoch());
    CHECK(kiyosi::exchange_calendar().trading_days_between(start, end) == 3);
    CHECK_THAT(kiyosi::exchange_calendar().trading_year_fraction(start, end),
               Catch::Matchers::WithinAbs(3.0 / 252.0, 1e-15));

    const auto schedule = kiyosi::make_observation_schedule(
        std::vector<kiyosi::date>{day(2025, 1, 2), day(2025, 1, 3)}, start, end,
        kiyosi::exchange_calendar());
    REQUIRE(schedule.has_value());
    const auto barrier = kiyosi::make_barrier_option(
        kiyosi::option_type::call, 100.0, start, end, 90.0, kiyosi::barrier_type::down_and_out,
        0.0, kiyosi::rebate_timing::at_expiry, kiyosi::observation_mode::scheduled, *schedule);
    REQUIRE(barrier.has_value());
    CHECK(barrier->schedule() == *schedule);
}

TEST_CASE("Effective dates schedules and SSE calendar semantics")
{
    const auto effective = day(2025, 1, 3);
    const auto expiry = day(2025, 2, 3);
    const auto option = kiyosi::make_european_call(100.0, effective, expiry);
    REQUIRE(option);
    CHECK(option->effective() == effective);
    CHECK_FALSE(kiyosi::make_european_call(100.0, expiry, effective));

    const auto fixed = kiyosi::make_fixed_interval_schedule(effective, day(2025, 1, 7), 1);
    REQUIRE(fixed);
    CHECK(fixed->dates() == std::vector<kiyosi::date>{day(2025, 1, 6), day(2025, 1, 7)});
    CHECK(kiyosi::make_fixed_interval_schedule(effective, day(2025, 1, 5), 10)->empty());

    const auto monthly = kiyosi::make_monthly_schedule(day(2025, 1, 2), day(2025, 4, 2), 2);
    REQUIRE(monthly);
    CHECK(monthly->dates() == std::vector<kiyosi::date>{day(2025, 3, 3), day(2025, 4, 2)});
    CHECK_FALSE(kiyosi::make_monthly_schedule(effective, expiry, 0));

    const auto sse = kiyosi::sse_calendar();
    CHECK(sse.annual_trading_days() == 243);
    CHECK_FALSE(sse.is_trading_day(day(1991, 2, 15)));
    CHECK_FALSE(sse.is_trading_day(day(2030, 9, 12)));
    CHECK_FALSE(sse.is_trading_day(day(2031, 1, 4)));
    CHECK(sse.is_trading_day(day(2031, 1, 2)));
}

}
