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
    const auto expiry_date = valuation + std::chrono::days{10};
    REQUIRE(expiry_date > valuation);
    REQUIRE(kiyosi::all_days_calendar().is_trading_day(expiry_date));
    REQUIRE_FALSE(kiyosi::weekdays_calendar().is_trading_day(day(2025, 1, 4)));

    auto custom = kiyosi::make_trading_calendar(
        [](kiyosi::Date value) { return value == day(2025, 1, 2) || value == day(2025, 1, 3); }, 2);
    REQUIRE(custom.has_value());
    const auto copied_calendar = *custom;
    REQUIRE(copied_calendar.is_trading_day(day(2025, 1, 2)));
    REQUIRE(copied_calendar.trading_days_per_year() == 2);

    const std::vector<kiyosi::Date> observation_dates{day(2025, 1, 2), day(2025, 1, 3)};
    REQUIRE(kiyosi::validate_observation_dates(observation_dates, valuation, expiry_date, copied_calendar).has_value());
    REQUIRE_FALSE(kiyosi::validate_observation_dates(
                      std::vector<kiyosi::Date>{day(2025, 1, 4)}, valuation, expiry_date, copied_calendar)
                      .has_value());
    REQUIRE_FALSE(kiyosi::validate_observation_dates(
                      std::vector<kiyosi::Date>{day(2025, 1, 2), day(2025, 1, 2)}, valuation, expiry_date, copied_calendar)
                      .has_value());

    auto parameters = kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    auto context = kiyosi::make_pricing_context(*parameters, 100.0, valuation, copied_calendar);
    REQUIRE(context.has_value());
    const auto context_copy = *context;
    REQUIRE(context_copy.calendar().is_trading_day(day(2025, 1, 2)));
}

TEST_CASE("Schedule errors distinguish dates, instrument life, and observation layout")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2025, 2, 1);
    const auto reversed_fixed = kiyosi::make_fixed_interval_schedule(end, start, std::chrono::days{1});
    REQUIRE_FALSE(reversed_fixed);
    CHECK(reversed_fixed.error().category == kiyosi::ErrorCategory::invalid_time_range);
    const auto reversed_monthly = kiyosi::make_monthly_schedule(end, start, 1);
    REQUIRE_FALSE(reversed_monthly);
    CHECK(reversed_monthly.error().category == kiyosi::ErrorCategory::invalid_time_range);
    const auto unsupported = kiyosi::make_fixed_interval_schedule(kiyosi::Date::max(), end, std::chrono::days{1});
    REQUIRE_FALSE(unsupported);
    CHECK(unsupported.error().category == kiyosi::ErrorCategory::invalid_date);

    const std::array unordered{end, start};
    const auto invalid_schedule = kiyosi::validate_date_schedule(unordered, start, end);
    REQUIRE_FALSE(invalid_schedule);
    CHECK(invalid_schedule.error().category == kiyosi::ErrorCategory::invalid_schedule);
    const std::array invalid_date{kiyosi::Date::max()};
    const auto unsupported_observation = kiyosi::validate_date_schedule(invalid_date, start, end);
    REQUIRE_FALSE(unsupported_observation);
    CHECK(unsupported_observation.error().category == kiyosi::ErrorCategory::invalid_date);
}

TEST_CASE("Nominal dates adjust in either direction without leaving the supported range")
{
    const auto calendar = kiyosi::weekdays_calendar();
    const auto nominal = day(2025, 1, 5);
    const auto following = calendar.adjust(nominal, kiyosi::BusinessDayConvention::following);
    const auto preceding = calendar.adjust(nominal, kiyosi::BusinessDayConvention::preceding);
    const auto unchanged = calendar.adjust(day(2025, 1, 6), kiyosi::BusinessDayConvention::following);
    REQUIRE(following);
    REQUIRE(preceding);
    REQUIRE(unchanged);
    CHECK(*following == day(2025, 1, 6));
    CHECK(*preceding == day(2025, 1, 3));
    CHECK(*unchanged == day(2025, 1, 6));
    const auto invalid = calendar.adjust(kiyosi::Date::max(), kiyosi::BusinessDayConvention::following);
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_date);
    const auto unknown = calendar.adjust(nominal, static_cast<kiyosi::BusinessDayConvention>(255));
    REQUIRE_FALSE(unknown);
    CHECK(unknown.error().category == kiyosi::ErrorCategory::invalid_parameter);
    const auto closed = *kiyosi::make_trading_calendar([](kiyosi::Date) { return false; }, 252);
    const auto last_supported = kiyosi::Date{std::chrono::year::max() / std::chrono::December / 31};
    const auto unavailable = closed.adjust(last_supported, kiyosi::BusinessDayConvention::following);
    REQUIRE_FALSE(unavailable);
    CHECK(unavailable.error().category == kiyosi::ErrorCategory::invalid_date);
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
        *kiyosi::make_bsm_parameters(0.01, 0.0, 0.2), 100.0, noon);
    REQUIRE(context.has_value());
    CHECK(context->valuation_date() == start);
    CHECK(context->valuation_time().time_since_epoch() == noon.time_since_epoch());
    CHECK(*kiyosi::weekdays_calendar().trading_days_between(start, end) == 3);
    CHECK_THAT(*kiyosi::weekdays_calendar().trading_year_fraction(start, end),
               Catch::Matchers::WithinAbs(3.0 / 252.0, 1e-15));

    const auto empty = kiyosi::weekdays_calendar().trading_days_between(start, start);
    REQUIRE(empty.has_value());
    CHECK(*empty == 0);
    const auto weekend_only = kiyosi::weekdays_calendar().trading_days_between(
        day(2025, 1, 4), day(2025, 1, 6));
    REQUIRE(weekend_only.has_value());
    CHECK(*weekend_only == 0);

    const auto reversed = kiyosi::weekdays_calendar().trading_days_between(end, start);
    REQUIRE_FALSE(reversed.has_value());
    CHECK(reversed.error().category == kiyosi::ErrorCategory::invalid_time_range);
    const auto reversed_fraction = kiyosi::weekdays_calendar().trading_year_fraction(end, start);
    REQUIRE_FALSE(reversed_fraction.has_value());
    CHECK(reversed_fraction.error().category == kiyosi::ErrorCategory::invalid_time_range);

    const auto invalid = kiyosi::weekdays_calendar().trading_days_between(kiyosi::Date::max(), end);
    REQUIRE_FALSE(invalid.has_value());
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_date);

    const auto first_supported = kiyosi::Date{std::chrono::year::min() / std::chrono::January / 1};
    const auto last_supported = kiyosi::Date{std::chrono::year::max() / std::chrono::December / 31};
    CHECK(kiyosi::is_supported_date(first_supported));
    CHECK(kiyosi::is_supported_date(last_supported));
    CHECK_FALSE(kiyosi::is_supported_date(first_supported - std::chrono::days{1}));
    CHECK_FALSE(kiyosi::is_supported_date(last_supported + std::chrono::days{1}));
    CHECK(kiyosi::date_of(kiyosi::start_of_day(first_supported)) == first_supported);
    CHECK(kiyosi::date_of(kiyosi::start_of_day(last_supported)) == last_supported);
    const auto full_range = kiyosi::year_fraction(
        kiyosi::start_of_day(first_supported), kiyosi::start_of_day(last_supported));
    REQUIRE(full_range.has_value());
    CHECK_THAT(*full_range,
               Catch::Matchers::WithinAbs(*kiyosi::year_fraction(first_supported, last_supported), 1e-9));

    const auto schedule = kiyosi::make_fixed_interval_schedule(
        start, day(2025, 1, 3), std::chrono::days{1}, kiyosi::weekdays_calendar());
    REQUIRE(schedule.has_value());
    const auto barrier = kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                      .strike = 100.0,
                                                      .effective_date = start,
                                                      .expiry_date = end,
                                                      .barrier_level = 90.0,
                                                      .barrier_type = kiyosi::BarrierType::down_and_out,
                                                      .rebate = 0.0,
                                                      .rebate_timing = kiyosi::RebateTiming::at_expiry,
                                                      .observation_mode = kiyosi::ObservationMode::scheduled,
                                                      .observation_dates = schedule->dates()});
    REQUIRE(barrier.has_value());
    CHECK(barrier->observation_schedule() == *schedule);
}

TEST_CASE("Effective dates, schedules, and SSE calendar semantics")
{
    const auto effective_date = day(2025, 1, 3);
    const auto expiry_date = day(2025, 2, 3);
    const auto option = kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, effective_date, expiry_date);
    REQUIRE(option);
    CHECK(option->effective_date() == effective_date);
    CHECK_FALSE(kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, expiry_date, effective_date));

    const auto fixed = kiyosi::make_fixed_interval_schedule(
        effective_date, day(2025, 1, 7), std::chrono::days{1});
    REQUIRE(fixed);
    CHECK(fixed->dates() == std::vector<kiyosi::Date>{day(2025, 1, 6), day(2025, 1, 7)});
    CHECK(kiyosi::make_fixed_interval_schedule(
              effective_date, day(2025, 1, 5), std::chrono::days{10})
              ->empty());

    const auto monthly = kiyosi::make_monthly_schedule(day(2025, 1, 2), day(2025, 4, 2), 2);
    REQUIRE(monthly);
    CHECK(monthly->dates() == std::vector<kiyosi::Date>{day(2025, 3, 3), day(2025, 4, 2)});
    const auto weekend_end = kiyosi::make_monthly_schedule(
        day(2025, 1, 1), day(2025, 3, 1), 1);
    REQUIRE(weekend_end);
    CHECK(weekend_end->dates() == std::vector<kiyosi::Date>{day(2025, 2, 3)});
    const auto month_end = kiyosi::make_monthly_schedule(
        day(2025, 1, 31), day(2025, 4, 30), 1);
    REQUIRE(month_end);
    CHECK(month_end->dates() == std::vector<kiyosi::Date>{
                                    day(2025, 2, 28), day(2025, 3, 31), day(2025, 4, 30)});
    CHECK_FALSE(kiyosi::make_monthly_schedule(effective_date, expiry_date, 0));
    CHECK(kiyosi::make_monthly_schedule(day(2025, 1, 1), day(2025, 12, 31), 786433)->empty());
    CHECK(kiyosi::make_monthly_schedule(day(32767, 11, 1), day(32767, 12, 31), 2)->empty());

    const auto sse = kiyosi::sse_calendar();
    CHECK(sse.trading_days_per_year() == 243);
    CHECK_FALSE(sse.is_trading_day(day(2031, 1, 4)));
    CHECK(sse.is_trading_day(day(2031, 1, 2)));
}

} // namespace
