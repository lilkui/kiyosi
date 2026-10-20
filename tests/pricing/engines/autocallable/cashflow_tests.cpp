#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <utility>
#include <vector>

#include <kiyosi/kiyosi.hpp>
#include <kiyosi/pricing/detail/autocallable_traits.hpp>

#include "pricing/engines/monte_carlo_autocallable.hpp"
#include "support/common.hpp"

namespace {
using namespace kiyosi;
using namespace kiyosi::detail;
using test::day;

constexpr auto start = day(2025, 1, 1), first = day(2025, 1, 11), middle = day(2025, 1, 21), end = day(2025, 1, 31);

PricingContext market(double spot, Timestamp valuation)
{
    const auto context = make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), spot, valuation, all_days_calendar());
    REQUIRE(context);
    return *context;
}

template <AutocallableProduct Note>
void check_prices(const Note& note, const PricingContext& context, double expected)
{
    const auto monte_carlo = make_monte_carlo_autocallable_engine({32, 7}).value();
    for (const auto& result : {monte_carlo.price(note, context), FiniteDifferenceAutocallableEngine{}.price(note, context), QuadratureAutocallableEngine{}.price(note, context)}) {
        REQUIRE(result);
        CHECK(*result == Catch::Approx(expected).epsilon(0).margin(1e-12));
    }
}

TEST_CASE("Phoenix fixed paths preserve conditional signed coupons and stop at knock-out", "[structured-cashflows]")
{
    for (const double coupon_rate : {0.365, -0.365}) {
        const auto later = end + std::chrono::days{10};
        const auto note = make_phoenix_option({.coupon_rate = coupon_rate, .initial_spot = 100, .knock_in_level = 80, .upper_strike = 100, .lower_strike = 60, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = later, .observations = {{first, 110, 0, 90}, {middle, 110, 0, 90}, {end, 110, 0, 90}, {later, 110, 0, 0}}}).value();
        const auto program = autocallable_program(note);
        auto state = autocallable_initial_state(note, market(100, start), program).path;
        const double coupon = coupon_rate > 0 ? 0.01 : -0.01;
        const double first_discount = std::exp(-0.04 * 10.0 / 365.0);
        const double last_discount = std::exp(-0.04 * 30.0 / 365.0);
        const std::array spots{90.0, std::nextafter(90.0, 0.0), 110.0, 1000.0};
        double payoff = -10;
        std::size_t processed = 0;
        for (const double spot : spots) {
            const auto index = processed++;
            REQUIRE(index < note.observations_view().size());
            const double discount = std::exp(-0.04 * 10.0 * static_cast<double>(index + 1) / 365.0);
            if (observe_autocallable_path(program, autocallable_event(note, index), {discount, std::log(discount)}, spot, state, payoff)) break;
            CHECK(state.coupons == Catch::Approx(coupon * first_discount).epsilon(0).margin(1e-14));
            CHECK_FALSE(state.knocked_in);
            CHECK(payoff == -10);
        }
        CHECK(processed == 3);
        CHECK(payoff == Catch::Approx(coupon * first_discount + (1.0 + coupon) * last_discount).epsilon(0).margin(1e-14));
        CHECK(state.coupons == Catch::Approx(coupon * first_discount).epsilon(0).margin(1e-14));
    }
}

TEST_CASE("Public snowball terms retain daily and historical knock-in through terminal settlement", "[structured-cashflows]")
{
    for (const auto mode : {KnockInObservationMode::every_trading_day, KnockInObservationMode::at_expiry}) {
        for (const auto history : {AutocallableBarrierState::none, AutocallableBarrierState::knocked_in}) {
            CAPTURE(mode, history);
            const auto snowball = make_snowball_option({.maturity_coupon_rate = 0.365, .initial_spot = 100, .knock_in_level = 80, .upper_strike = 100, .lower_strike = 60, .knock_in_observation_mode = mode, .barrier_state = history, .effective_date = start, .expiry_date = end, .observations = {{end, 110, 0.365}}}).value();
            const auto ternary = make_ternary_snowball_option({.maturity_coupon_rate = 0.365, .minimum_coupon_rate = -0.365, .knock_in_level = 80, .knock_in_observation_mode = mode, .barrier_state = history, .effective_date = start, .expiry_date = end, .observations = {{end, 110, 0.365}}}).value();
            const bool knocked_in = mode == KnockInObservationMode::every_trading_day || history == AutocallableBarrierState::knocked_in;
            const double discount = std::exp(-0.04 * 30.0 / 365.0);
            const auto check = [&](const auto& note, double expected) {
                const auto program = autocallable_program(note);
                auto state = autocallable_initial_state(note, market(100, start), program).path;
                double payoff = -10;
                CHECK_FALSE(observe_autocallable_path(program, {}, {}, 70, state, payoff));
                CHECK(state.knocked_in == knocked_in);
                CHECK(state.coupons == 0);
                CHECK_FALSE(observe_autocallable_path(program, autocallable_event(note, 0), {discount, std::log(discount)}, 90, state, payoff));
                CHECK(state.coupons == 0);
                CHECK(settle_autocallable_path(program, {discount, std::log(discount)}, 90, state) == Catch::Approx(expected * discount).epsilon(0).margin(1e-14));
            };
            check(snowball, knocked_in ? 0.9 : 1.03);
            check(ternary, knocked_in ? 0.97 : 1.03);
        }
    }
}

TEST_CASE("Public autocallable current knock-outs pay only their contractual coupon", "[structured-cashflows]")
{
    const std::vector<AutocallableObservation> observations{{first, 100, -0.365}, {middle, 100, 100}, {end, 100, 100}};
    const auto context = market(100, first);
    check_prices(make_binary_snowball_option({.maturity_coupon_rate = 5, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = observations}).value(), context, 0.99);
    check_prices(make_snowball_option({.maturity_coupon_rate = 5, .initial_spot = 100, .knock_in_level = 80, .upper_strike = 100, .lower_strike = 60, .barrier_state = AutocallableBarrierState::knocked_in, .effective_date = start, .expiry_date = end, .observations = observations}).value(), context, 0.99);
    check_prices(make_ternary_snowball_option({.maturity_coupon_rate = 5, .minimum_coupon_rate = -1, .knock_in_level = 80, .barrier_state = AutocallableBarrierState::knocked_in, .effective_date = start, .expiry_date = end, .observations = observations}).value(), context, 0.99);
    for (const double barrier : {100.0, std::nextafter(100.0, 200.0)}) {
        const auto note = make_phoenix_option({.coupon_rate = -0.365, .initial_spot = 100, .knock_in_level = 80, .upper_strike = 100, .lower_strike = 60, .barrier_state = AutocallableBarrierState::knocked_in, .effective_date = start, .expiry_date = end, .observations = {{first, 100, 0, barrier}, {end, 100, 0, 0}}}).value();
        check_prices(note, context, barrier == 100 ? 0.99 : 1.0);
    }
}

TEST_CASE("Public historical knock-in and terminal caps agree across autocallable engines", "[structured-cashflows]")
{
    for (const auto& [spot, expected] : std::array{std::pair{50.0, 0.85}, std::pair{90.0, 1.15}, std::pair{105.0, 1.25}}) {
        CAPTURE(spot);
        const auto note = make_snowball_option({.maturity_coupon_rate = 5, .initial_spot = 100, .knock_in_level = 80, .upper_strike = 100, .lower_strike = 60, .barrier_state = AutocallableBarrierState::knocked_in, .principal_ratio = 1.25, .effective_date = start, .expiry_date = end, .observations = {{end, 110, 100}}}).value();
        check_prices(note, market(spot, end), expected);
        const auto phoenix = make_phoenix_option({.coupon_rate = -0.365, .initial_spot = 100, .knock_in_level = 80, .upper_strike = 100, .lower_strike = 60, .barrier_state = AutocallableBarrierState::knocked_in, .principal_ratio = 1.25, .effective_date = start, .expiry_date = end, .observations = {{first, 110, 0, 90}, {end, 110, 0, 90}}}).value();
        check_prices(phoenix, market(spot, end), expected - (spot >= 90 ? 0.02 : 0));
    }
}
} // namespace
