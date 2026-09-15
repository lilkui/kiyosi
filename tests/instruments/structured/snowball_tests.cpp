#include <catch2/catch_test_macros.hpp>
#include <catch2/catch_approx.hpp>

#include <chrono>
#include <limits>
#include <type_traits>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::test::day;
}

TEST_CASE("Snowball factory rejects invalid schedules and supports signed coupon replacement")
{
    static_assert(!std::is_constructible_v<kiyosi::BinarySnowballOption, std::vector<double>, double, double,
                                           std::vector<double>, double, double, std::vector<kiyosi::date>,
                                           kiyosi::barrier_touch_status, double, kiyosi::date, kiyosi::date>);

    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto note = kiyosi::make_snowball_option(kiyosi::SnowballTerms{
        .knock_out_coupon_rates = {-0.1},
        .maturity_coupon_rate = -0.05,
        .initial_price = 100.0,
        .knock_in_price = 60.0,
        .knock_out_prices = {110.0},
        .upper_strike = 100.0,
        .lower_strike = 60.0,
        .observations = {expiry},
        .frequency = kiyosi::observation_frequency::daily,
        .touch_status = kiyosi::barrier_touch_status::none,
        .principal_ratio = 1.0,
        .effective = effective,
        .expiry = expiry});
    REQUIRE(note);
    const auto replaced = note->with_coupon_rate(-0.08);
    REQUIRE(replaced);
    CHECK(replaced->maturity_coupon_rate() == -0.08);
    CHECK_FALSE(note->with_coupon_rate(std::numeric_limits<double>::infinity()));
    CHECK(kiyosi::make_snowball_option(kiyosi::SnowballTerms{
              .knock_out_coupon_rates = {0.1},
              .maturity_coupon_rate = 0.05,
              .initial_price = 100.0,
              .knock_in_price = 60.0,
              .knock_out_prices = {110.0},
              .upper_strike = 100.0,
              .lower_strike = 60.0,
              .observations = {expiry, effective},
              .frequency = kiyosi::observation_frequency::daily,
              .touch_status = kiyosi::barrier_touch_status::none,
              .principal_ratio = 1.0,
              .effective = effective,
              .expiry = expiry})
              .error()
              .category == kiyosi::error_category::invalid_schedule);
}

TEST_CASE("Named Snowball factories build DerivaSharp variants")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const std::vector<kiyosi::date> observations{day(2025, 4, 1), day(2025, 7, 1), expiry};

    const auto standard = kiyosi::make_standard_snowball(0.1, 100.0, 70.0, 105.0, observations, effective, expiry);
    const auto step_down = kiyosi::make_step_down_snowball(0.1, 100.0, 70.0, 110.0, 5.0, observations, effective, expiry);
    const auto both_down = kiyosi::make_both_down_snowball(0.1, 0.01, 100.0, 70.0, 110.0, 5.0, observations, effective, expiry);
    const auto dual = kiyosi::make_dual_coupon_snowball(0.1, 0.03, 100.0, 70.0, 105.0, observations, effective, expiry);
    const auto parachute = kiyosi::make_parachute_snowball(0.1, 100.0, 70.0, 105.0, 90.0, observations, effective, expiry);
    const auto otm = kiyosi::make_otm_snowball(0.1, 100.0, 70.0, 105.0, 110.0, observations, effective, expiry);
    const auto capped = kiyosi::make_loss_capped_snowball(0.1, 100.0, 70.0, 105.0, 80.0, observations, effective, expiry);
    const auto european = kiyosi::make_european_snowball(0.1, 100.0, 70.0, 105.0, observations, effective, expiry);

    REQUIRE(standard);
    REQUIRE(step_down);
    REQUIRE(both_down);
    REQUIRE(dual);
    REQUIRE(parachute);
    REQUIRE(otm);
    REQUIRE(capped);
    REQUIRE(european);
    CHECK(step_down->knock_out_prices()[1] == Catch::Approx(105.0));
    CHECK(both_down->knock_out_coupon_rates()[1] == Catch::Approx(0.09));
    CHECK(parachute->knock_out_prices().back() == Catch::Approx(90.0));
    CHECK(otm->upper_strike() == Catch::Approx(110.0));
    CHECK(capped->lower_strike() == Catch::Approx(80.0));
    CHECK(european->knock_in_frequency() == kiyosi::observation_frequency::at_expiry);
    CHECK(dual->maturity_coupon_rate() == Catch::Approx(0.03));
}
