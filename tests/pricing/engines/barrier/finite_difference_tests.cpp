#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <catch2/generators/catch_generators.hpp>

#include <algorithm>
#include <cmath>
#include <optional>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "pricing/detail/fd_grid.hpp"
#include "pricing/detail/fd_scheme.hpp"
#include "pricing/detail/math.hpp"
#include "support/common.hpp"

TEST_CASE("Continuous up-and-out rollback preserves full-grid prices and Greeks", "[fd-barrier-trim]")
{
    using namespace kiyosi;
    using namespace kiyosi::detail;
    const auto scheme = GENERATE(FiniteDifferenceScheme::explicit_euler, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::crank_nicolson);
    const auto type = GENERATE(OptionType::call, OptionType::put);
    const auto timing = GENERATE(RebateTiming::at_hit, RebateTiming::at_expiry);
    const int steps = GENERATE(3, 4, 31, 200);
    const double barrier = GENERATE(120.0, 121.1);
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 119.25, start);
    const auto option = *make_barrier_option({.option_type = type, .strike = 100.0,
                                             .effective_date = start, .expiry_date = end,
                                             .barrier_level = barrier, .barrier_type = BarrierType::up_and_out,
                                             .rebate = 1.5, .rebate_timing = timing});
    const FiniteDifferenceSettings settings{steps, 2'000, scheme, 480.0};
    const auto space = *make_spatial_grid(settings, 480.0, {119.25, 100.0, barrier});
    std::vector<double> full(space.size());
    for (int index = 0; index <= steps; ++index) {
        const double asset = space.spacing * index;
        full[index] = asset >= barrier ? option.rebate()
                                      : std::max(type == OptionType::call ? asset - 100.0 : 100.0 - asset, 0.0);
    }
    const auto rebate = [&](double tau) {
        return timing == RebateTiming::at_hit ? option.rebate() : scaled_exponential(option.rebate(), -0.04 * tau);
    };
    // Keep the original full solve as the reference for pruning fixed rows.
    const auto marched = march_backward(
        make_finite_difference_time_grid(1.0, settings.time_step_count),
        DiffusionParameters{0.04, 0.01, 0.2, scheme_theta(scheme)}, full,
        [&](double tau) { return Boundaries{type == OptionType::put ? 100.0 * std::exp(-0.04 * tau) : 0.0, rebate(tau)}; },
        [](std::vector<double>&, double, double) {},
        [&](int index, double tau) -> std::optional<double> {
            return space.spacing * index >= barrier ? std::optional{rebate(tau)} : std::nullopt;
        });
    REQUIRE(marched);
    const FiniteDifferenceBarrierEngine engine{settings};
    const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma, Greek::speed});
    REQUIRE(result);
    CHECK(result->price() == Catch::Approx(space.interpolate(full, context.spot_price())).epsilon(1e-12));
    CHECK(test::greek_value(*result, Greek::delta) == Catch::Approx(space.delta(full, context.spot_price())).epsilon(1e-12).margin(1e-14));
    CHECK(test::greek_value(*result, Greek::gamma) == Catch::Approx(space.gamma(full, context.spot_price())).epsilon(1e-12).margin(1e-14));
    const double h = symmetric_shift(context.spot_price(), NumericalShiftSettings{}.spot_shift);
    const double speed = (space.gamma(full, context.spot_price() + h) - space.gamma(full, context.spot_price() - h)) / (2.0 * h);
    CHECK(test::greek_value(*result, Greek::speed) == Catch::Approx(speed).epsilon(1e-12).margin(1e-14));
}

TEST_CASE("Continuous up-and-out trimming retains explicit stability validation", "[fd-barrier-trim]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.1), 100.0, start);
    const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0,
                                             .effective_date = start, .expiry_date = end,
                                             .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out});
    const auto result = FiniteDifferenceBarrierEngine{200, 50, FiniteDifferenceScheme::explicit_euler}.price(option, context);
    REQUIRE_FALSE(result);
    CHECK(result.error().category == ErrorCategory::invalid_parameter);
}
