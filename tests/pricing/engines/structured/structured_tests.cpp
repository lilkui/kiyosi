#include <catch2/catch_test_macros.hpp>
#include <catch2/catch_approx.hpp>
#include <chrono>
#include <limits>
#include <type_traits>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"

namespace {

using kiyosi::test::day;

template <typename Instrument>
void check_structured_refinement(const Instrument& instrument, const kiyosi::PricingContext& context)
{
    for (const auto scheme : {kiyosi::finite_difference_scheme::explicit_euler,
                              kiyosi::finite_difference_scheme::implicit_euler,
                              kiyosi::finite_difference_scheme::crank_nicolson}) {
        const auto coarse = kiyosi::FiniteDifferenceStructuredEngine<Instrument>{{40, 512, scheme}}.price(instrument, context);
        const auto fine = kiyosi::FiniteDifferenceStructuredEngine<Instrument>{{80, 1024, scheme}}.price(instrument, context);
        REQUIRE(coarse);
        REQUIRE(fine);
        const double coarse_value = *coarse->get(kiyosi::risk_measure::price);
        const double fine_value = *fine->get(kiyosi::risk_measure::price);
        CHECK(std::isfinite(coarse_value));
        CHECK(std::isfinite(fine_value));
        CHECK(fine_value != coarse_value);
    }
}

TEST_CASE("Structured factories enforce validation and signed coupon replacement")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    static_assert(!std::is_constructible_v<kiyosi::Accumulator, double, double, double, double, double,
                                           kiyosi::date, kiyosi::date>);
    static_assert(!std::is_constructible_v<kiyosi::BinarySnowballOption, std::vector<double>, double, double,
                                           std::vector<double>, double, double, std::vector<kiyosi::date>,
                                           kiyosi::barrier_touch_status, double, kiyosi::date, kiyosi::date>);

    CHECK(kiyosi::make_accumulator(100.0, 110.0, 1.0, 2.0, 0.0, expiry, effective).error().category ==
          kiyosi::error_category::invalid_schedule);
    const auto note = kiyosi::make_snowball_option(
        {-0.1}, -0.05, 100.0, 60.0, {110.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    REQUIRE(note);
    const auto replaced = note->with_coupon_rate(-0.08);
    REQUIRE(replaced);
    CHECK(replaced->maturity_coupon_rate() == -0.08);
    CHECK_FALSE(note->with_coupon_rate(std::numeric_limits<double>::infinity()));
    CHECK(kiyosi::make_snowball_option(
              {0.1}, 0.05, 100.0, 60.0, {110.0}, 100.0, 60.0,
              {expiry, effective}, kiyosi::observation_frequency::daily,
              kiyosi::barrier_touch_status::none, 1.0, effective, expiry)
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

TEST_CASE("Phoenix expiry settlement applies state and final observations")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto market = [&](double spot) {
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::MonteCarloStructuredEngine<std::remove_cvref_t<decltype(note)>>{{32, 7}}.price(note, market(spot));
        REQUIRE(result);
        return *result->get(kiyosi::risk_measure::price);
    };

    const auto phoenix = *kiyosi::make_phoenix_option(
        0.08, 100.0, 80.0, {100.0}, {90.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::at_expiry, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    CHECK(price(phoenix, 100.0) == Catch::Approx(9.0));
    CHECK(price(phoenix, 80.0) == Catch::Approx(1.0));
    const auto phoenix_up = *kiyosi::make_phoenix_option(
        0.08, 100.0, 80.0, {100.0}, {90.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::up,
        1.0, effective, expiry);
    CHECK(price(phoenix_up, 100.0) == 0.0);
}

TEST_CASE("Snowball expiry settlement applies state and final observations")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto market = [&](double spot) {
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::MonteCarloStructuredEngine<std::remove_cvref_t<decltype(note)>>{{32, 7}}.price(note, market(spot));
        REQUIRE(result);
        return *result->get(kiyosi::risk_measure::price);
    };
    const auto snowball = *kiyosi::make_snowball_option(
        {0.10}, 0.05, 100.0, 80.0, {100.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::at_expiry, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    CHECK(price(snowball, 100.0) == Catch::Approx(1.10));
    CHECK(price(snowball, 80.0) == Catch::Approx(1.05));
    CHECK(price(snowball, 59.0) == Catch::Approx(0.60));
    const auto snowball_down = *kiyosi::make_snowball_option(
        {0.10}, 0.05, 100.0, 80.0, {110.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::down,
        1.0, effective, expiry);
    CHECK(price(snowball_down, 70.0) == Catch::Approx(0.70));
}

TEST_CASE("Binary snowball expiry settlement applies final observations")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto market = [&](double spot) {
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::MonteCarloStructuredEngine<std::remove_cvref_t<decltype(note)>>{{32, 7}}.price(note, market(spot));
        REQUIRE(result);
        return *result->get(kiyosi::risk_measure::price);
    };
    const auto binary = *kiyosi::make_binary_snowball_option(
        {0.10}, 0.05, 100.0, {100.0}, 100.0, 60.0, {expiry},
        kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    CHECK(price(binary, 100.0) == Catch::Approx(1.10));
    CHECK(price(binary, 90.0) == Catch::Approx(1.05));
}

TEST_CASE("Ternary snowball expiry settlement applies final observations")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto market = [&](double spot) {
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::MonteCarloStructuredEngine<std::remove_cvref_t<decltype(note)>>{{32, 7}}.price(note, market(spot));
        REQUIRE(result);
        return *result->get(kiyosi::risk_measure::price);
    };
    const auto ternary = *kiyosi::make_ternary_snowball_option(
        {0.10}, 0.05, 0.02, 100.0, 80.0, {100.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::at_expiry, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    CHECK(price(ternary, 100.0) == Catch::Approx(1.10));
    CHECK(price(ternary, 80.0) == Catch::Approx(1.05));
    CHECK(price(ternary, 79.0) == Catch::Approx(1.02));
}

TEST_CASE("Accumulator expiry settlement agrees across pricing engines")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto market = [&](double spot) {
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::MonteCarloStructuredEngine<std::remove_cvref_t<decltype(note)>>{{32, 7}}.price(note, market(spot));
        REQUIRE(result);
        return *result->get(kiyosi::risk_measure::price);
    };
    const auto accumulator = *kiyosi::make_accumulator(100.0, 110.0, 1.0, 2.0, 3.0, effective, expiry);
    CHECK(price(accumulator, 90.0) == Catch::Approx(-50.0));
    const auto accumulator_fd = kiyosi::FiniteDifferenceAccumulatorEngine{}.price(accumulator, market(90.0));
    REQUIRE(accumulator_fd);
    CHECK(*accumulator_fd->get(kiyosi::risk_measure::price) == Catch::Approx(-50.0));
}

TEST_CASE("Structured Monte Carlo processes valuation-date observation events once")
{
    const auto effective = day(2025, 1, 1);
    const auto valuation = day(2025, 7, 1);
    const auto expiry = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.0, 0.0, 1e-12), 100.0, valuation);
    const auto monte_carlo_price = [&](const auto& instrument) {
        using Instrument = std::remove_cvref_t<decltype(instrument)>;
        const auto result = kiyosi::MonteCarloStructuredEngine<Instrument>{{32, 7}}.price(instrument, context);
        REQUIRE(result);
        return *result->get(kiyosi::risk_measure::price);
    };
    const auto note = *kiyosi::make_binary_snowball_option(
        {99.0, 10.0, 0.10}, 0.05, 100.0, {90.0, 90.0, 99.0}, 100.0, 60.0,
        {effective, valuation, expiry}, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const kiyosi::MonteCarloBinarySnowballEngine engine{{32, 7}};
    const auto first = engine.price(note, context);
    const auto second = engine.price(note, context);
    REQUIRE(first);
    REQUIRE(second);
    CHECK(*first->get(kiyosi::risk_measure::price) == *second->get(kiyosi::risk_measure::price));
    CHECK(*first->get(kiyosi::risk_measure::price) ==
          Catch::Approx(1.0 + 10.0 * kiyosi::year_fraction(effective, valuation).value()).margin(1e-10));
    const auto snowball = *kiyosi::make_snowball_option(
        {99.0, 10.0, 0.10}, 0.05, 100.0, 80.0, {90.0, 90.0, 99.0}, 100.0, 60.0,
        {effective, valuation, expiry}, kiyosi::observation_frequency::at_expiry,
        kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    CHECK(monte_carlo_price(snowball) == Catch::Approx(1.0 + 10.0 * kiyosi::year_fraction(effective, valuation).value()));

    const auto ternary = *kiyosi::make_ternary_snowball_option(
        {99.0, 10.0, 0.10}, 0.05, 0.02, 100.0, 80.0, {90.0, 90.0, 99.0}, 100.0, 60.0,
        {effective, valuation, expiry}, kiyosi::observation_frequency::at_expiry,
        kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    CHECK(monte_carlo_price(ternary) == Catch::Approx(1.0 + 10.0 * kiyosi::year_fraction(effective, valuation).value()));

    const auto phoenix = *kiyosi::make_phoenix_option(
        0.08, 100.0, 80.0, {90.0, 90.0, 99.0}, {90.0, 90.0, 90.0}, 100.0, 60.0,
        {effective, valuation, expiry}, kiyosi::observation_frequency::at_expiry,
        kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    CHECK(monte_carlo_price(phoenix) == Catch::Approx(9.0));
}

TEST_CASE("Accumulator finite-difference engine refines its event-aware BSM grid")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const auto accumulator = *kiyosi::make_accumulator(100.0, 110.0, 1.0, 2.0, 3.0, effective, expiry);
    check_structured_refinement(accumulator, context);
}

TEST_CASE("Phoenix finite-difference engine refines its event-aware BSM grid")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const std::vector<kiyosi::date> observations{day(2025, 4, 1), day(2025, 7, 1),
                                                 day(2025, 10, 1), expiry};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const auto phoenix = *kiyosi::make_phoenix_option(
        0.02, 100.0, 75.0, knock_outs, {90.0, 90.0, 90.0, 90.0}, 100.0, 60.0,
        observations, kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    check_structured_refinement(phoenix, context);
}

TEST_CASE("Snowball finite-difference engine refines its event-aware BSM grid")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const std::vector<kiyosi::date> observations{day(2025, 4, 1), day(2025, 7, 1),
                                                 day(2025, 10, 1), expiry};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto snowball = *kiyosi::make_snowball_option(
        coupons, 0.08, 100.0, 75.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    check_structured_refinement(snowball, context);
}

TEST_CASE("Binary snowball finite-difference engine refines its event-aware BSM grid")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const std::vector<kiyosi::date> observations{day(2025, 4, 1), day(2025, 7, 1),
                                                 day(2025, 10, 1), expiry};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto binary = *kiyosi::make_binary_snowball_option(
        coupons, 0.08, 100.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    check_structured_refinement(binary, context);
}

TEST_CASE("Ternary snowball finite-difference engine refines its event-aware BSM grid")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const std::vector<kiyosi::date> observations{day(2025, 4, 1), day(2025, 7, 1),
                                                 day(2025, 10, 1), expiry};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto ternary = *kiyosi::make_ternary_snowball_option(
        coupons, 0.08, 0.02, 100.0, 75.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    check_structured_refinement(ternary, context);
}

TEST_CASE("Finite-difference binary snowball engine rejects unstable explicit grids")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const std::vector<kiyosi::date> observations{day(2025, 4, 1), day(2025, 7, 1),
                                                 day(2025, 10, 1), expiry};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto binary = *kiyosi::make_binary_snowball_option(
        coupons, 0.08, 100.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    CHECK_FALSE(kiyosi::FiniteDifferenceBinarySnowballEngine{{40, 1, kiyosi::finite_difference_scheme::explicit_euler}}
                    .price(binary, context));
}

TEST_CASE("Finite-difference phoenix engine rejects domains below the barrier")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const std::vector<kiyosi::date> observations{day(2025, 4, 1), day(2025, 7, 1),
                                                 day(2025, 10, 1), expiry};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const auto phoenix = *kiyosi::make_phoenix_option(
        0.02, 100.0, 75.0, knock_outs, {90.0, 90.0, 90.0, 90.0}, 100.0, 60.0,
        observations, kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    CHECK_FALSE(kiyosi::FiniteDifferencePhoenixEngine{{40, 512, kiyosi::finite_difference_scheme::crank_nicolson, 110.0}}
                    .price(phoenix, context));
}

}
