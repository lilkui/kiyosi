#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <type_traits>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::AutocallableBarrierState;
using kiyosi::test::day;

const auto effective_date = day(2025, 1, 1);
const auto expiry_date = day(2026, 1, 1); // Exactly one ACT/365 year.

auto market(double spot, kiyosi::Date valuation, double rate = 0.0, double sigma = 0.2)
{
    const auto calendar = kiyosi::make_trading_calendar([](kiyosi::Date) { return true; }, 365);
    REQUIRE(calendar);
    const auto parameters = kiyosi::make_bsm_parameters(rate, rate, sigma);
    REQUIRE(parameters);
    const auto context = kiyosi::make_pricing_context(*parameters, spot, valuation, *calendar);
    REQUIRE(context);
    return *context;
}

template <typename Instrument>
void check_known_price(const Instrument& instrument, const kiyosi::PricingContext& context,
                       double expected, double tolerance = 1e-12)
{
    const auto check = [&](const auto& engine) {
        const auto result = engine.price(instrument, context);
        REQUIRE(result);
        const auto price = result;
        REQUIRE(price);
        CHECK(*price == Catch::Approx(expected).margin(tolerance).epsilon(0.0));
    };
    // The explicit domain puts all integer levels on nodes for the limiting cases.
    const kiyosi::FiniteDifferenceSettings grid{
        400, 400, kiyosi::FiniteDifferenceScheme::crank_nicolson, 400.0};
    if constexpr (std::is_same_v<Instrument, kiyosi::Accumulator>) {
        {
            INFO("FD");
            check(kiyosi::FiniteDifferenceAccumulatorEngine{grid});
        }
        {
            INFO("MC");
            check(kiyosi::MonteCarloAccumulatorEngine{{64, 73}});
        }
    } else {
        {
            INFO("FD");
            check(kiyosi::FiniteDifferenceAutocallableEngine<Instrument>{grid});
        }
        {
            INFO("MC");
            check(kiyosi::MonteCarloAutocallableEngine<Instrument>{{64, 73}});
        }
    }
}

auto snowball(AutocallableBarrierState status)
{
    return kiyosi::make_snowball_option({.knock_out_coupon_rates = {0.20}, .maturity_coupon_rate = 0.12, .initial_spot = 100.0, .knock_in_level = 80.0, .knock_out_levels = {120.0}, .upper_strike = 100.0, .lower_strike = 60.0, .observation_dates = {expiry_date}, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = status, .effective_date = effective_date, .expiry_date = expiry_date});
}

auto ternary(AutocallableBarrierState status)
{
    return kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = {0.20}, .maturity_coupon_rate = 0.12, .minimum_coupon_rate = 0.03, .knock_in_level = 80.0, .knock_out_levels = {120.0}, .observation_dates = {expiry_date}, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = status, .effective_date = effective_date, .expiry_date = expiry_date});
}

auto binary(AutocallableBarrierState status)
{
    return kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.20}, .maturity_coupon_rate = 0.12, .knock_out_levels = {120.0}, .observation_dates = {expiry_date}, .barrier_state = status, .effective_date = effective_date, .expiry_date = expiry_date});
}

auto phoenix(AutocallableBarrierState status)
{
    return kiyosi::make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .knock_out_levels = {120.0}, .coupon_barrier_levels = {90.0}, .upper_strike = 100.0, .lower_strike = 60.0, .observation_dates = {expiry_date}, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = status, .effective_date = effective_date, .expiry_date = expiry_date});
}
} // namespace

TEST_CASE("FD-MC snowball families match independent expiry_date payoffs", "[cross-validation]")
{
    struct Case {
        double spot;
        double snowball;
        double ternary;
        double binary;
    };
    // KI is strict (<80), KO inclusive (>=120); loss is capped at 40/100.
    const std::array cases{
        Case{59.0, 0.60, 1.03, 1.12}, Case{60.0, 0.60, 1.03, 1.12},
        Case{61.0, 0.61, 1.03, 1.12}, Case{79.0, 0.79, 1.03, 1.12},
        Case{80.0, 1.12, 1.12, 1.12}, Case{81.0, 1.12, 1.12, 1.12},
        Case{119.0, 1.12, 1.12, 1.12}, Case{120.0, 1.20, 1.20, 1.20},
        Case{121.0, 1.20, 1.20, 1.20}};
    const auto standard = snowball(AutocallableBarrierState::none);
    const auto three_way = ternary(AutocallableBarrierState::none);
    const auto two_way = binary(AutocallableBarrierState::none);
    REQUIRE(standard);
    REQUIRE(three_way);
    REQUIRE(two_way);
    for (const auto& row : cases) {
        CAPTURE(row.spot);
        const auto context = market(row.spot, expiry_date);
        {
            INFO("snowball");
            check_known_price(*standard, context, row.snowball);
        }
        {
            INFO("ternary");
            check_known_price(*three_way, context, row.ternary);
        }
        {
            INFO("binary");
            check_known_price(*two_way, context, row.binary);
        }
    }
}

TEST_CASE("FD-MC phoenix matches independent coupon and loss boundaries", "[cross-validation]")
{
    const auto note = phoenix(AutocallableBarrierState::none);
    REQUIRE(note);
    // One full-year observation pays 0.08 normalized principal units.
    for (const auto [spot, expected] : std::array{
             std::array{59.0, 0.60}, std::array{60.0, 0.60}, std::array{61.0, 0.61},
             std::array{79.0, 0.79}, std::array{80.0, 1.0}, std::array{81.0, 1.0},
             std::array{89.0, 1.0}, std::array{90.0, 1.08}, std::array{91.0, 1.08},
             std::array{119.0, 1.08}, std::array{120.0, 1.08}, std::array{121.0, 1.08}}) {
        CAPTURE(spot);
        check_known_price(*note, market(spot, expiry_date), expected);
    }
}

TEST_CASE("FD-MC historical knock-in survives recovery and knock-out extinguishes notes",
          "[cross-validation]")
{
    const auto standard = snowball(AutocallableBarrierState::knocked_in);
    const auto three_way = ternary(AutocallableBarrierState::knocked_in);
    const auto bird = phoenix(AutocallableBarrierState::knocked_in);
    REQUIRE(standard);
    REQUIRE(three_way);
    REQUIRE(bird);
    // Recovery through the upper strike removes loss but does not restore a snowball coupon.
    for (const auto [spot, principal] : std::array{
             std::array{99.0, 0.99}, std::array{100.0, 1.0}, std::array{101.0, 1.0}}) {
        CAPTURE(spot);
        const auto context = market(spot, expiry_date);
        check_known_price(*standard, context, principal);
        check_known_price(*three_way, context, 1.03);
        check_known_price(*bird, context, principal + 0.08);
    }
    const auto check_extinguished = [](const auto& result) {
        REQUIRE(result);
        const auto before_first = kiyosi::MonteCarloAutocallableEngine<std::remove_cvref_t<decltype(*result)>>{{64, 73}}
                                      .price(*result, market(100.0, effective_date));
        REQUIRE_FALSE(before_first);
        CHECK(before_first.error().category == kiyosi::ErrorCategory::invalid_option);
    };
    check_extinguished(snowball(AutocallableBarrierState::knocked_out));
    check_extinguished(ternary(AutocallableBarrierState::knocked_out));
    check_extinguished(binary(AutocallableBarrierState::knocked_out));
    check_extinguished(phoenix(AutocallableBarrierState::knocked_out));
}

TEST_CASE("FD-MC accumulator matches independent expiry_date quantity accounting", "[cross-validation]")
{
    const auto option = kiyosi::make_accumulator({100.0, 110.0, 1.0, 2.0, 3.0, effective_date, expiry_date});
    REQUIRE(option);
    // Below strike buy two; above strike buy one; at KO settle existing three only.
    for (const auto [spot, expected] : std::array{
             std::array{99.0, -5.0}, std::array{100.0, 0.0}, std::array{101.0, 4.0},
             std::array{109.0, 36.0}, std::array{110.0, 30.0}, std::array{111.0, 33.0}}) {
        CAPTURE(spot);
        check_known_price(*option, market(spot, expiry_date), expected);
    }
}

TEST_CASE("FD-MC pre-expiry_date prices approach independently known constant-path limits",
          "[cross-validation]")
{
    // Zero volatility is outside the model API. At 1e-8 these spots cannot practically
    // cross any barrier; tolerance also covers the residual MC path displacement.
    const auto context = market(100.0, effective_date, 0.0, 1e-8);
    const auto check = [&](const auto& note, double expected) {
        REQUIRE(note);
        check_known_price(*note, context, expected, 1e-6);
    };
    check(snowball(AutocallableBarrierState::none), 1.12);
    check(ternary(AutocallableBarrierState::none), 1.12);
    check(binary(AutocallableBarrierState::none), 1.12);
    check(phoenix(AutocallableBarrierState::none), 1.08);

    const auto option = kiyosi::make_accumulator(
        {100.0, 110.0, 1.0, 2.0, 3.0, effective_date, day(2025, 1, 6)});
    REQUIRE(option);
    // Six trading observations (including valuation and expiry_date): (3 + 6*2)*(90-100).
    check_known_price(*option, market(90.0, effective_date, 0.0, 1e-8), -150.0, 1e-5);
}

TEST_CASE("FD-MC binary snowball discounts a known fixed terminal cashflow", "[cross-validation]")
{
    // The only observation is expiry_date and both outcomes pay 1.12, for every simulated path.
    const auto note = kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.12}, .maturity_coupon_rate = 0.12, .knock_out_levels = {120.0}, .observation_dates = {expiry_date}, .effective_date = effective_date, .expiry_date = expiry_date});
    REQUIRE(note);
    check_known_price(*note, market(100.0, effective_date, 0.05), 1.12 * std::exp(-0.05), 1e-8);
}

TEST_CASE("FD-MC accumulator current knock-out settles immediately", "[cross-validation]")
{
    const auto option = kiyosi::make_accumulator(
        {100.0, 110.0, 1.0, 2.0, 3.0, effective_date, day(2025, 1, 6)});
    REQUIRE(option);
    check_known_price(*option, market(110.0, effective_date, 0.05), 30.0);
}

TEST_CASE("FD-MC accumulator future knock-out settles on first trading day",
          "[cross-validation]")
{
    const auto valuation = day(2025, 1, 4); // Saturday: no current observation.
    const auto parameters = kiyosi::make_bsm_parameters(0.05, 0.05, 1e-8);
    REQUIRE(parameters);
    const auto context = kiyosi::make_pricing_context(
        *parameters, 111.0, valuation, kiyosi::weekdays_calendar());
    REQUIRE(context);
    // Monday's KO precedes its purchase and cancels Tuesday's purchase. Nothing
    // accrues over the weekend; existing holdings settle two days after valuation.
    const double discount = std::exp(-0.05 * 2.0 / 365.0);
    const kiyosi::FiniteDifferenceAccumulatorEngine fd{
        {400, 400, kiyosi::FiniteDifferenceScheme::crank_nicolson, 400.0}};
    const kiyosi::MonteCarloAccumulatorEngine mc{{64, 73}};
    for (const double prior_quantity : {0.0, 3.0}) {
        CAPTURE(prior_quantity);
        const auto option = kiyosi::make_accumulator(
            {100.0, 110.0, 1.0, 2.0, prior_quantity, effective_date, day(2025, 1, 7)});
        REQUIRE(option);
        const double expected = prior_quantity * 11.0 * discount;
        const auto fd_result = fd.price(*option, *context);
        const auto mc_result = mc.price(*option, *context);
        REQUIRE(fd_result);
        REQUIRE(mc_result);
        const auto fd_price = fd_result;
        const auto mc_price = mc_result;
        REQUIRE(fd_price);
        REQUIRE(mc_price);
        CHECK(*fd_price == Catch::Approx(expected).margin(1e-7).epsilon(0.0));
        CHECK(*mc_price == Catch::Approx(expected).margin(1e-5).epsilon(0.0));
    }
}

TEST_CASE("FD-MC phoenix pays its terminal observation coupon exactly once", "[cross-validation]")
{
    const auto start = day(2025, 1, 6);
    const auto maturity = start + std::chrono::days{91};
    const auto note = kiyosi::make_phoenix_option({.coupon_rate = 0.0025, .initial_spot = 100.0, .knock_in_level = 80.0, .knock_out_levels = {120.0}, .coupon_barrier_levels = {90.0}, .upper_strike = 100.0, .lower_strike = 60.0, .observation_dates = {maturity}, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .effective_date = start, .expiry_date = maturity});
    REQUIRE(note);
    const auto context = market(100.0, start, 0.0, 1e-8);
    // 91/365 and this subdivision count previously introduced a second endpoint
    // nearly equal to maturity. One observation owes principal 1 plus the accrued coupon.
    const auto fd = kiyosi::FiniteDifferencePhoenixEngine{
        {400, 1600, kiyosi::FiniteDifferenceScheme::crank_nicolson, 400.0}}
                        .price(*note, context);
    const auto mc = kiyosi::MonteCarloPhoenixEngine{{64, 73}}.price(*note, context);
    REQUIRE(fd);
    REQUIRE(mc);
    for (const auto& result : {*fd, *mc}) {
        CHECK(result == Catch::Approx(1.0 + 0.0025 * 91.0 / 365.0).margin(1e-6).epsilon(0.0));
    }
}

TEST_CASE("FD-MC phoenix coupons preserve price scale and annual accrual", "[cross-validation]")
{
    const auto middle = day(2025, 7, 1);
    for (const double scale : {100.0, 1000.0}) {
        for (const bool split : {false, true}) {
            const std::vector<kiyosi::Date> dates = split ? std::vector{middle, expiry_date}
                                                          : std::vector{expiry_date};
            const auto note = kiyosi::make_phoenix_option({.coupon_rate = 0.08,
                                                           .initial_spot = scale,
                                                           .knock_in_level = 0.5 * scale,
                                                           .knock_out_levels = std::vector(dates.size(), 2.0 * scale),
                                                           .coupon_barrier_levels = std::vector(dates.size(), 0.9 * scale),
                                                           .upper_strike = scale,
                                                           .lower_strike = 0.0,
                                                           .observation_dates = dates,
                                                           .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry,
                                                           .effective_date = effective_date,
                                                           .expiry_date = expiry_date});
            REQUIRE(note);
            CAPTURE(scale, split);
            const auto context = market(scale, effective_date, 0.0, 1e-8);
            const auto check = [&](const auto& engine) {
                const auto result = engine.price(*note, context);
                REQUIRE(result);
                CHECK(*result == Catch::Approx(1.08).margin(1e-6).epsilon(0.0));
            };
            check(kiyosi::FiniteDifferencePhoenixEngine{
                {400, 400, kiyosi::FiniteDifferenceScheme::crank_nicolson, 4.0 * scale}});
            check(kiyosi::MonteCarloPhoenixEngine{{64, 73}});
        }
    }
}
