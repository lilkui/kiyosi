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
    CHECK_FALSE(scheduled.barrier_terms().is_monitored_on(effective_date));
    CHECK(scheduled.barrier_terms().mean_observation_year_fraction() > 0.0);
}
