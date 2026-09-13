#include <catch2/catch_test_macros.hpp>

#include <chrono>
#include <optional>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
kiyosi::date standard_expiry()
{
    return kiyosi::date{std::chrono::year{2026} / 1 / 6};
}
} // namespace

TEST_CASE("Barrier public constructors reject invalid contracts")
{
    const auto expiry = standard_expiry();
    const auto effective = expiry - std::chrono::days{365};
    CHECK(kiyosi::make_barrier_option(kiyosi::option_type::call, -1.0, effective, expiry, 90.0,
                                      kiyosi::barrier_type::down_and_in)
              .error()
              .category == kiyosi::error_category::invalid_strike);
    CHECK(kiyosi::make_barrier_option(kiyosi::option_type::call, 100.0, effective, expiry, 90.0,
                                      kiyosi::barrier_type::down_and_in, 10.0,
                                      kiyosi::rebate_timing::at_hit)
              .error()
              .category == kiyosi::error_category::invalid_option);
    CHECK(kiyosi::make_binary_barrier_option(std::nullopt, 100.0, effective, expiry, 90.0,
                                             kiyosi::barrier_type::down_and_out, 10.0, false,
                                             kiyosi::rebate_timing::at_hit)
              .error()
              .category == kiyosi::error_category::invalid_option);
    CHECK(kiyosi::make_binary_barrier_option(std::nullopt, 100.0, effective, expiry, 90.0,
                                             kiyosi::barrier_type::down_and_in, 10.0, false,
                                             kiyosi::rebate_timing::at_expiry,
                                             kiyosi::observation_mode::scheduled)
              .error()
              .category == kiyosi::error_category::invalid_schedule);
}

TEST_CASE("Barrier terms expose shared monitoring and knock predicates")
{
    const auto expiry = standard_expiry();
    const auto effective = expiry - std::chrono::days{365};
    const auto up_out = *kiyosi::make_barrier_option(kiyosi::option_type::call, 100.0, effective, expiry,
                                                     120.0, kiyosi::barrier_type::up_and_out);
    const auto& terms = up_out.barrier_terms();
    CHECK(terms.is_up());
    CHECK_FALSE(terms.is_knock_in());
    CHECK(terms.is_continuous());
    CHECK(terms.breaches(120.0));
    CHECK_FALSE(terms.breaches(119.9));
    CHECK(terms.monitors(kiyosi::start_of_day(effective)));
    CHECK(terms.observation_interval() == 0.0);

    const std::vector<kiyosi::date> observations{effective + std::chrono::days{30},
                                                 effective + std::chrono::days{60}};
    const auto scheduled = *kiyosi::make_binary_barrier_option(
        std::nullopt, 100.0, effective, expiry, 90.0, kiyosi::barrier_type::down_and_out, 10.0, false,
        kiyosi::rebate_timing::at_expiry, kiyosi::observation_mode::scheduled, observations);
    CHECK_FALSE(scheduled.barrier_terms().is_up());
    CHECK_FALSE(scheduled.barrier_terms().is_continuous());
    CHECK(scheduled.barrier_terms().monitors(kiyosi::start_of_day(observations.front())));
    CHECK_FALSE(scheduled.barrier_terms().monitors(kiyosi::start_of_day(effective)));
    CHECK(scheduled.barrier_terms().observation_interval() > 0.0);
}
