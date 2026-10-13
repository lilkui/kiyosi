#include <catch2/catch_test_macros.hpp>

#include <chrono>
#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
kiyosi::Date standard_expiry()
{
    return kiyosi::Date{std::chrono::year{2026} / 1 / 6};
}
} // namespace

TEST_CASE("Barrier terms expose shared monitoring and knock predicates")
{
    const auto expiry_date = standard_expiry();
    const auto effective_date = expiry_date - std::chrono::days{365};
    const auto up_out = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                      .strike = 100.0,
                                                      .effective_date = effective_date,
                                                      .expiry_date = expiry_date,
                                                      .barrier_level = 120.0,
                                                      .barrier_type = kiyosi::BarrierType::up_and_out});
    const auto& terms = up_out.barrier_terms();
    CHECK(terms.is_up());
    CHECK_FALSE(terms.is_knock_in());
    CHECK(terms.is_continuous());
    CHECK(terms.is_breached_by(120.0));
    CHECK_FALSE(terms.is_breached_by(119.9));
    CHECK(terms.is_monitored_on(effective_date));
    CHECK(terms.is_monitored_on(expiry_date));
    CHECK_FALSE(terms.is_monitored_on(effective_date - std::chrono::days{1}));
    CHECK_FALSE(terms.is_monitored_on(expiry_date + std::chrono::days{1}));
    CHECK(terms.mean_observation_year_fraction() == 0.0);

    const std::vector<kiyosi::Date> observation_dates{effective_date + std::chrono::days{30},
                                                      effective_date + std::chrono::days{60}};
    const auto scheduled = *kiyosi::make_cash_no_touch_down(
        effective_date, expiry_date, 90.0, 10.0, kiyosi::ObservationMode::scheduled, observation_dates);
    CHECK_FALSE(scheduled.barrier_terms().is_up());
    CHECK_FALSE(scheduled.barrier_terms().is_continuous());
    CHECK(scheduled.barrier_terms().is_monitored_on(observation_dates.front()));
    const auto fixing = kiyosi::start_of_day(observation_dates.front());
    CHECK(scheduled.barrier_terms().is_monitored_at(fixing));
    CHECK_FALSE(scheduled.barrier_terms().is_monitored_at(fixing - std::chrono::microseconds{1}));
    CHECK_FALSE(scheduled.barrier_terms().is_monitored_at(fixing + std::chrono::microseconds{1}));
    CHECK_FALSE(scheduled.barrier_terms().is_monitored_at(fixing + std::chrono::hours{12}));
    CHECK(terms.is_monitored_at(fixing + std::chrono::hours{12}));
    CHECK_FALSE(scheduled.barrier_terms().is_monitored_on(effective_date));
    CHECK(scheduled.barrier_terms().mean_observation_year_fraction() > 0.0);

    const auto before_first_observation = kiyosi::start_of_day(effective_date + std::chrono::days{15});
    CHECK(scheduled.barrier_terms().was_touched_before(before_first_observation) == kiyosi::Result<bool>{false});
    CHECK(scheduled.barrier_terms().was_touched_before(fixing) == kiyosi::Result<bool>{false});
    const auto missing_history = scheduled.barrier_terms().was_touched_before(fixing + std::chrono::microseconds{1});
    REQUIRE_FALSE(missing_history);
    CHECK(missing_history.error().category == kiyosi::ErrorCategory::invalid_parameter);
    const auto impossible = *kiyosi::make_cash_no_touch_down(
        effective_date, expiry_date, 90.0, 10.0, kiyosi::ObservationMode::scheduled,
        observation_dates, kiyosi::BarrierTouchState::touched);
    const auto impossible_result = impossible.barrier_terms().was_touched_before(before_first_observation);
    REQUIRE_FALSE(impossible_result);
    CHECK(impossible_result.error().category == kiyosi::ErrorCategory::invalid_option);
}

TEST_CASE("Barrier monitoring state preserves observation boundaries and history", "[barrier-state]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1), first = test::day(2025, 2, 3);
    const auto last = test::day(2025, 3, 3), expiry = test::day(2025, 4, 1);
    const auto make_option = [&](ObservationMode mode, std::optional<BarrierTouchState> history) {
        return *make_cash_no_touch_down(start, expiry, 90.0, 10.0, mode,
                                        mode == ObservationMode::scheduled ? std::vector{first, last} : std::vector<Date>{}, history);
    };
    SECTION("Only the exact final observation can add a current touch")
    {
        const auto option = make_option(ObservationMode::scheduled, BarrierTouchState::untouched);
        for (const int offset : {-1, 0, 1}) {
            const auto state = detail::barrier_monitoring_state(
                option.barrier_terms(), start_of_day(last) + std::chrono::microseconds{offset}, 89.0);
            REQUIRE(state);
            CHECK_FALSE(state->touched_before);
            CHECK(state->touched == (offset == 0));
            CHECK(state->finished == (offset >= 0));
        }
    }
    SECTION("Prior touches survive a currently unbreached spot")
    {
        const auto option = make_option(ObservationMode::scheduled, BarrierTouchState::touched);
        const auto state = detail::barrier_monitoring_state(option.barrier_terms(), start_of_day(last), 100.0);
        REQUIRE(state);
        CHECK(state->touched_before);
        CHECK(state->touched);
        CHECK(state->finished);
    }
    SECTION("Continuous monitoring includes intraday spot and is not schedule-finished")
    {
        const auto option = make_option(ObservationMode::continuous, BarrierTouchState::untouched);
        const auto state = detail::barrier_monitoring_state(
            option.barrier_terms(), start_of_day(last) + std::chrono::hours{12}, 90.0);
        REQUIRE(state);
        CHECK_FALSE(state->touched_before);
        CHECK(state->touched);
        CHECK_FALSE(state->finished);
    }
    SECTION("Missing and impossible history retain their error categories")
    {
        const auto missing = make_option(ObservationMode::scheduled, std::nullopt);
        const auto state = detail::barrier_monitoring_state(missing.barrier_terms(), start_of_day(last), 89.0);
        REQUIRE_FALSE(state);
        CHECK(state.error().category == ErrorCategory::invalid_parameter);
        const auto impossible = make_option(ObservationMode::scheduled, BarrierTouchState::touched);
        const auto early = detail::barrier_monitoring_state(impossible.barrier_terms(), start_of_day(first), 89.0);
        REQUIRE_FALSE(early);
        CHECK(early.error().category == ErrorCategory::invalid_option);
    }
}
