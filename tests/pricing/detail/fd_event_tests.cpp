#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::test::day;

constexpr auto effective_date = day(2025, 1, 1);
constexpr auto expiry_date = day(2026, 1, 1);
} // namespace

TEST_CASE("Finite-difference current events use the actual spot", "[audit-fixes]")
{
    const auto event = day(2025, 1, 2);
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto note = *kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.2, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{event, 120.0, 0.1}, {expiry_date, 130.0, 0.1}}});
    const auto accumulator = *kiyosi::make_accumulator(
        {100.0, 120.0, 1.0, 2.0, 3.0, effective_date, expiry_date});
    for (const double spot : {120.0, 120.1}) {
        const auto context = *kiyosi::make_pricing_context(parameters, spot, event);
        for (const double upper : {499.0, 501.0}) {
            CAPTURE(spot, upper);
            const kiyosi::FiniteDifferenceSettings grid{
                200, 200, kiyosi::FiniteDifferenceScheme::crank_nicolson, upper};
            const auto called = kiyosi::make_finite_difference_autocallable_engine(grid).value().price(note, context);
            REQUIRE(called);
            CHECK(*called == Catch::Approx(1.0 + 0.1 / 365.0).epsilon(0.0).margin(1e-12));
            const auto settled = kiyosi::make_finite_difference_accumulator_engine(grid).value().price(accumulator, context);
            REQUIRE(settled);
            CHECK(*settled == Catch::Approx(3.0 * (spot - 100.0)).epsilon(0.0).margin(1e-12));
        }
    }
}

TEST_CASE("Finite-difference current accrual and knock-in precede continuation", "[audit-fixes]")
{
    const auto event = day(2025, 1, 2);
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto closed_today = *kiyosi::make_trading_calendar(
        [event](kiyosi::Date date) { return date != event && kiyosi::weekdays_calendar().is_trading_day(date); }, 252);
    const kiyosi::FiniteDifferenceSettings grid{
        200, 200, kiyosi::FiniteDifferenceScheme::crank_nicolson, 499.0};
    const auto accumulator_engine = kiyosi::make_finite_difference_accumulator_engine(grid).value();
    for (const double spot : {99.9, 100.0, 100.1}) {
        CAPTURE(spot);
        const auto context = *kiyosi::make_pricing_context(parameters, spot, event);
        const auto after_purchase = *kiyosi::make_pricing_context(parameters, spot, event, closed_today);
        const auto before = *kiyosi::make_accumulator(
            {100.0, 120.0, 1.0, 2.0, 3.0, effective_date, expiry_date});
        const auto after = *kiyosi::make_accumulator(
            {100.0, 120.0, 1.0, 2.0, spot < 100.0 ? 5.0 : 4.0, effective_date, expiry_date});
        const auto actual = accumulator_engine.price(before, context);
        const auto expected = accumulator_engine.price(after, after_purchase);
        REQUIRE(actual);
        REQUIRE(expected);
        CHECK(*actual == *expected);
    }
    const auto snowball = [&](kiyosi::AutocallableBarrierState state) {
        return *kiyosi::make_snowball_option({.maturity_coupon_rate = 0.2, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = state, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 120.0, 0.1}}});
    };
    const auto context = *kiyosi::make_pricing_context(parameters, 79.9, event);
    const auto engine = kiyosi::make_finite_difference_autocallable_engine(grid).value();
    const auto current = engine.price(snowball(kiyosi::AutocallableBarrierState::none), context);
    const auto prior = engine.price(snowball(kiyosi::AutocallableBarrierState::knocked_in), context);
    REQUIRE(current);
    REQUIRE(prior);
    CHECK(*current == *prior);
}

TEST_CASE("Finite-difference current coupons use the actual barrier branch", "[audit-fixes]")
{
    const auto event = day(2025, 1, 2);
    const auto note = [&](double barrier) {
        return *kiyosi::make_phoenix_option({.coupon_rate = 0.1, .initial_spot = 100.0, .knock_in_level = 60.0, .upper_strike = 100.0, .lower_strike = 0.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{event, 120.0, 0.0, barrier}, {expiry_date, 130.0, 0.0, 80.0}}});
    };
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2), 80.0, event);
    const auto engine = kiyosi::make_finite_difference_autocallable_engine({200, 200, kiyosi::FiniteDifferenceScheme::crank_nicolson, 499.0}).value();
    const auto paid = engine.price(note(80.0), context);
    const auto missed = engine.price(note(80.1), context);
    REQUIRE(paid);
    REQUIRE(missed);
    CHECK(*paid - *missed == Catch::Approx(0.1 / 365.0).margin(1e-12).epsilon(0.0));
}
