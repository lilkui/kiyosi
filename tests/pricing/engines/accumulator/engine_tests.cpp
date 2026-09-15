#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <cmath>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::test::day;
}

TEST_CASE("Accumulator expiry settlement agrees across pricing engines")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto market = [&](double spot) {
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2), spot, expiry);
    };
    const auto accumulator = *kiyosi::make_accumulator({.strike = 100.0,
                                                        .knock_out = 110.0,
                                                        .daily_quantity = 1.0,
                                                        .acceleration = 2.0,
                                                        .accumulated_quantity = 3.0,
                                                        .effective = effective,
                                                        .expiry = expiry});

    const auto monte_carlo = kiyosi::MonteCarloAccumulatorEngine{{32, 7}}.price(accumulator, market(90.0));
    REQUIRE(monte_carlo);
    CHECK(*monte_carlo->get(kiyosi::risk_measure::price) == Catch::Approx(-50.0));

    const auto finite_difference =
        kiyosi::FiniteDifferenceAccumulatorEngine{}.price(accumulator, market(90.0));
    REQUIRE(finite_difference);
    CHECK(*finite_difference->get(kiyosi::risk_measure::price) == Catch::Approx(-50.0));
}

TEST_CASE("Accumulator finite-difference engine refines its event-aware BSM grid")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const auto accumulator = *kiyosi::make_accumulator({.strike = 100.0,
                                                        .knock_out = 110.0,
                                                        .daily_quantity = 1.0,
                                                        .acceleration = 2.0,
                                                        .accumulated_quantity = 3.0,
                                                        .effective = effective,
                                                        .expiry = expiry});
    for (const auto scheme : {kiyosi::finite_difference_scheme::explicit_euler,
                              kiyosi::finite_difference_scheme::implicit_euler,
                              kiyosi::finite_difference_scheme::crank_nicolson}) {
        const auto coarse = kiyosi::FiniteDifferenceAccumulatorEngine{{40, 512, scheme}}.price(accumulator, context);
        const auto fine = kiyosi::FiniteDifferenceAccumulatorEngine{{80, 1024, scheme}}.price(accumulator, context);
        REQUIRE(coarse);
        REQUIRE(fine);
        const double coarse_value = *coarse->get(kiyosi::risk_measure::price);
        const double fine_value = *fine->get(kiyosi::risk_measure::price);
        CHECK(std::isfinite(coarse_value));
        CHECK(std::isfinite(fine_value));
        CHECK(fine_value != coarse_value);
    }
}
