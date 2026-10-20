#include <catch2/catch_test_macros.hpp>
#include <catch2/generators/catch_generators.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <optional>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "pricing/detail/fd_scheme.hpp"
#include "pricing/detail/fd_grid.hpp"
#include "pricing/detail/fd_event.hpp"
#include "pricing/detail/fd_concentrated_grid.hpp"
#include "pricing/detail/fd_damping.hpp"

TEST_CASE("Finite-difference projection supports one and three independent layers", "[fd-components]")
{
    const kiyosi::detail::SpatialGrid grid{4.0, 1.0, 4};
    const std::array breaks{0.75, 1.0, 1.25};
    const auto value_at = [](double coordinate) {
        const double first = coordinate < 0.75 ? 0.0 : coordinate < 1.0 ? 2.0
                                                   : coordinate < 1.25  ? 6.0
                                                                        : 10.0;
        return std::array{first, coordinate < 1.0 ? 5.0 : 13.0, 2.0 * coordinate + 3.0};
    };
    const auto single = kiyosi::detail::project_finite_difference_event<1>(grid, breaks, 1, [&](double coordinate) {
        return std::array{value_at(coordinate)[0]};
    });
    CHECK_THAT(single[0], Catch::Matchers::WithinAbs(4.5, 1e-14));
    const auto triple = kiyosi::detail::project_finite_difference_event<3>(grid, breaks, 1, value_at);
    CHECK_THAT(triple[0], Catch::Matchers::WithinAbs(4.5, 1e-14));
    CHECK_THAT(triple[1], Catch::Matchers::WithinAbs(9.0, 1e-14));
    CHECK_THAT(triple[2], Catch::Matchers::WithinAbs(5.0, 1e-14));
    const auto smooth = kiyosi::detail::project_finite_difference_event<3>(grid, {}, 1, value_at);
    CHECK((smooth == std::array{6.0, 13.0, 5.0}));
}

TEST_CASE("Concentrated finite-difference grids preserve anchors and resolution", "[fd-components]")
{
    using namespace kiyosi::detail;
    const SpatialGrid original{400.0, 400.0 / 128.0, 128};
    const auto grid = concentrate_spatial_grid(original, {110.0, 80.0, 100.0}, 0.1);
    REQUIRE(grid.nodes.size() == 129);
    CHECK(grid.size() == grid.nodes.size());
    CHECK(grid.asset_step_count == 128);
    CHECK(grid.upper == original.upper);
    CHECK(grid.nodes.front() == 0.0);
    CHECK(grid.nodes.back() == 400.0);
    CHECK(grid.sinh_scale == 0.0);
    for (std::size_t index = 1; index < grid.nodes.size(); ++index) {
        CHECK(std::isfinite(grid.nodes[index]));
        CHECK(grid.nodes[index] > grid.nodes[index - 1]);
        CHECK(grid.asset_at(index) == grid.nodes[index]);
    }
    for (const double level : {80.0, 100.0, 110.0}) {
        const auto anchor = std::ranges::lower_bound(grid.nodes, level);
        REQUIRE(anchor != grid.nodes.end());
        REQUIRE(*anchor == level);
        const double left = *anchor - *(anchor - 1), right = *(anchor + 1) - *anchor;
        CHECK(left < original.spacing);
        CHECK_THAT(left, Catch::Matchers::WithinAbs(right, 1e-12));
    }
    const auto filtered = concentrate_spatial_grid(original,
                                                   {-1.0, 0.0, std::nextafter(0.0, 1.0), 80.0, 80.0, std::nextafter(80.0, 100.0), 100.0, 110.0, std::nextafter(400.0, 0.0), 400.0, 401.0}, 0.1);
    CHECK(filtered.nodes == grid.nodes);
}

TEST_CASE("Concentrated finite-difference grids retain the original when anchors cannot fit", "[fd-components]")
{
    using namespace kiyosi::detail;
    const SpatialGrid original{4.0, 1.0, 4, 0.5};
    for (const auto& levels : {std::vector<double>{}, {-1.0, 0.0, 4.0, 5.0}, {1.0, 2.0, 3.0}}) {
        const auto grid = concentrate_spatial_grid(original, levels, 0.1);
        CHECK(grid.nodes.empty());
        CHECK(grid.upper == original.upper);
        CHECK(grid.spacing == original.spacing);
        CHECK(grid.asset_step_count == original.asset_step_count);
        CHECK(grid.sinh_scale == original.sinh_scale);
    }
}

TEST_CASE("Rannacher damping consumes intervals and restarts independently", "[fd-components]")
{
    using namespace kiyosi;
    detail::RannacherDamping damping{2};
    for (int interval = 0; interval < 2; ++interval) {
        const auto substeps = damping.next(0.5);
        CHECK(substeps.count == 2);
        CHECK(substeps.theta == 1.0);
    }
    const auto normal = damping.next(0.5);
    CHECK(normal.count == 1);
    CHECK(normal.theta == 0.5);
    damping.restart();
    CHECK(damping.next(0.5).count == 2);
    damping.restart();
    CHECK(damping.next(0.5).count == 2);
    CHECK(damping.next(0.5).count == 2);
    CHECK(damping.next(0.5).count == 1);
    detail::RannacherDamping independent{1};
    CHECK(independent.next(0.5).count == 2);
    CHECK(independent.next(0.5).count == 1);
}

TEST_CASE("Rannacher damping preserves Euler schemes and can be disabled", "[fd-components]")
{
    using namespace kiyosi;
    for (const double theta : {0.0, 1.0}) {
        detail::RannacherDamping damping{2};
        for (int interval = 0; interval < 4; ++interval) {
            const auto substeps = damping.next(theta);
            CHECK(substeps.count == 1);
            CHECK(substeps.theta == theta);
        }
        damping.restart();
        CHECK(damping.next(theta).count == 1);
    }
    detail::RannacherDamping disabled{0};
    for (int interval = 0; interval < 3; ++interval) {
        const auto substeps = disabled.next(0.5);
        CHECK(substeps.count == 1);
        CHECK(substeps.theta == 0.5);
        disabled.restart();
    }
}

TEST_CASE("Initial finite-difference time refinement preserves exact event suffixes", "[fd-components]")
{
    using namespace kiyosi::detail;
    const double adjacent = std::nextafter(0.25, 1.0);
    const FiniteDifferenceTimeGrid original{{0.0, 0.25, adjacent, 1.0}, {0.25, adjacent - 0.25, 1.0 - adjacent}};
    auto grid = original;
    refine_initial_finite_difference_time_grid(grid, 0.25, 4);
    CHECK(grid.times == std::vector<double>{0.0, 0.0625, 0.125, 0.1875, 0.25, adjacent, 1.0});
    CHECK(grid.time_steps == std::vector<double>{0.0625, 0.0625, 0.0625, 0.0625, adjacent - 0.25, 1.0 - adjacent});
    for (const int minimum : {1, 3, 4}) {
        const auto refined = grid;
        refine_initial_finite_difference_time_grid(grid, 0.25, minimum);
        CHECK(grid.times == refined.times);
        CHECK(grid.time_steps == refined.time_steps);
    }
    grid = {{0.0, 0.5, 1.0}, {0.5, 0.5}};
    refine_initial_finite_difference_time_grid(grid, 1.0, 4);
    CHECK(grid.times == std::vector<double>{0.0, 0.25, 0.5, 0.75, 1.0});
    CHECK(grid.time_steps == std::vector<double>{0.25, 0.25, 0.25, 0.25});
}

TEST_CASE("Finite-difference event projection integrates every break in both layers", "[autocallable-accuracy]")
{
    const kiyosi::detail::SpatialGrid grid{4.0, 1.0, 4};
    const std::array breaks{0.75, 1.0, 1.25};
    const auto value_at = [](double coordinate) {
        const double first = coordinate < 0.75 ? 0.0 : coordinate < 1.0 ? 2.0
                                                   : coordinate < 1.25  ? 6.0
                                                                        : 10.0;
        return std::array{first, coordinate < 1.0 ? 5.0 : 13.0};
    };
    const auto result = kiyosi::detail::project_finite_difference_event<2>(grid, breaks, 1, value_at);
    CHECK_THAT(result[0], Catch::Matchers::WithinAbs(4.5, 1e-14));
    CHECK_THAT(result[1], Catch::Matchers::WithinAbs(9.0, 1e-14));
}

TEST_CASE("Finite-difference event projection leaves smooth cells and endpoints exact", "[autocallable-accuracy]")
{
    const kiyosi::detail::SpatialGrid grid{4.0, 1.0, 4};
    int calls = 0;
    const auto value_at = [&](double coordinate) {
        ++calls;
        return std::array{coordinate * coordinate, 2.0 * coordinate + 3.0};
    };
    SECTION("No break means no repeated smoothing")
    {
        const auto result = kiyosi::detail::project_finite_difference_event<2>(grid, {}, 1, value_at);
        CHECK((result == std::array{1.0, 5.0}));
        CHECK(calls == 1);
    }
    SECTION("Breaks at cell edges do not enter the cell")
    {
        const std::array breaks{0.5, 1.5};
        const auto result = kiyosi::detail::project_finite_difference_event<2>(grid, breaks, 1, value_at);
        CHECK((result == std::array{1.0, 5.0}));
        CHECK(calls == 1);
    }
    SECTION("Boundary values stay pointwise even near breaks")
    {
        const std::array breaks{0.1, 3.9};
        for (const std::size_t index : {std::size_t{0}, grid.size() - 1}) {
            const auto result = kiyosi::detail::project_finite_difference_event<2>(grid, breaks, index, value_at);
            const double coordinate = grid.spacing * static_cast<double>(index);
            CHECK(result[0] == coordinate * coordinate);
            CHECK(result[1] == 2.0 * coordinate + 3.0);
        }
        CHECK(calls == 2);
    }
}

TEST_CASE("Finite-difference event projection weights stretched cells in solver coordinates", "[autocallable-accuracy]")
{
    const kiyosi::detail::SpatialGrid grid{100.0 * std::sinh(0.4), 0.1, 4, 100.0};
    const std::array breaks{0.175};
    const auto result = kiyosi::detail::project_finite_difference_event<2>(grid, breaks, 2, [&](double coordinate) {
        return std::array{coordinate < 0.175 ? 2.0 : 7.0, grid.asset_of_coordinate(coordinate)};
    });
    CHECK_THAT(result[0], Catch::Matchers::WithinAbs(5.75, 1e-13));
    const double expected_asset_average = 100.0 * (std::cosh(0.25) - std::cosh(0.15)) / 0.1;
    CHECK_THAT(result[1], Catch::Matchers::WithinAbs(expected_asset_average, 1e-7));
}

TEST_CASE("Finite-difference event projection respects nonuniform control volumes", "[autocallable-accuracy]")
{
    const kiyosi::detail::SpatialGrid grid{4.0, 1.0, 4, 0.0, {0.0, 0.4, 1.0, 2.0, 4.0}};
    CHECK(grid.nearest_node(0.0) == 0);
    CHECK(grid.nearest_node(0.3) == 1);
    CHECK(grid.nearest_node(1.5) == 2);
    CHECK(grid.nearest_node(std::nextafter(1.5, 2.0)) == 3);
    CHECK(grid.nearest_node(4.0) == 4);
    const std::array breaks{0.9, 1.0, 1.2};
    const auto result = kiyosi::detail::project_finite_difference_event<2>(grid, breaks, 2, [](double coordinate) {
        const double first = coordinate < 0.9 ? 0.0 : coordinate < 1.0 ? 2.0
                                                  : coordinate < 1.2   ? 6.0
                                                                       : 10.0;
        return std::array{first, coordinate < 1.0 ? 5.0 : 13.0};
    });
    CHECK_THAT(result[0], Catch::Matchers::WithinAbs(5.5, 1e-13));
    CHECK_THAT(result[1], Catch::Matchers::WithinAbs(10.0, 1e-13));
}

TEST_CASE("Nonuniform finite-difference splines retain physical derivatives and cached consistency", "[autocallable-accuracy]")
{
    const kiyosi::detail::SpatialGrid grid{4.0, 1.0, 3, 0.0, {0.0, 1.0, 2.0, 4.0}};
    // Integrating continuous curvature {0,2,-1,0} from initial slope 1 gives this natural spline.
    const std::array values{0.0, 4.0 / 3.0, 23.0 / 6.0, 7.5};
    CHECK_THAT(grid.linear_interpolate(values, 1.5), Catch::Matchers::WithinAbs(31.0 / 12.0, 1e-14));
    std::vector<double> curvature;
    for (const double t : {0.1, 0.5, 0.9}) {
        CAPTURE(t);
        const double spot = 1.0 + t;
        const double expected = 4.0 / 3.0 + 2.0 * t + t * t - 0.5 * t * t * t;
        const double delta = 2.0 + 2.0 * t - 1.5 * t * t;
        const double gamma = 2.0 - 3.0 * t;
        CHECK_THAT(grid.interpolate(values, spot, curvature), Catch::Matchers::WithinAbs(expected, 1e-14));
        CHECK_THAT(grid.delta(values, spot, curvature), Catch::Matchers::WithinAbs(delta, 1e-14));
        CHECK_THAT(grid.gamma(values, spot, curvature), Catch::Matchers::WithinAbs(gamma, 1e-14));
        CHECK_THAT(grid.delta(values, spot), Catch::Matchers::WithinAbs(delta, 1e-14));
        CHECK_THAT(grid.gamma(values, spot), Catch::Matchers::WithinAbs(gamma, 1e-14));
    }
    for (std::size_t index = 0; index < grid.size(); ++index)
        CHECK(grid.interpolate(values, grid.asset_at(index)) == values[index]);
    const std::array affine{3.0, 5.0, 7.0, 11.0};
    for (const double spot : {0.0, 0.25, 1.0, 1.5, 3.0, 4.0}) {
        CHECK_THAT(grid.interpolate(affine, spot), Catch::Matchers::WithinAbs(3.0 + 2.0 * spot, 1e-14));
        CHECK_THAT(grid.linear_interpolate(affine, spot), Catch::Matchers::WithinAbs(3.0 + 2.0 * spot, 1e-14));
        CHECK_THAT(grid.delta(affine, spot), Catch::Matchers::WithinAbs(2.0, 1e-14));
        CHECK_THAT(grid.gamma(affine, spot), Catch::Matchers::WithinAbs(0.0, 1e-14));
    }
}

TEST_CASE("Nonuniform spline overshoot is bounded with consistent Greeks", "[autocallable-accuracy]")
{
    const kiyosi::detail::SpatialGrid grid{4.0, 1.0, 3, 0.0, {0.0, 1.0, 2.0, 4.0}};
    for (const double sign : {-1.0, 1.0}) {
        const std::array values{1.0 - sign, 1.0, 1.0, 1.0};
        std::vector<double> curvature;
        for (const double spot : {1.25, 1.5, 1.75}) {
            CAPTURE(sign, spot);
            CHECK(grid.interpolate(values, spot, curvature) == 1.0);
            CHECK(grid.delta(values, spot, curvature) == 0.0);
            CHECK(grid.gamma(values, spot, curvature) == 0.0);
            CHECK(grid.delta(values, spot) == 0.0);
            CHECK(grid.gamma(values, spot) == 0.0);
            constexpr double bump = 1e-3;
            const double low = grid.interpolate(values, spot - bump);
            const double high = grid.interpolate(values, spot + bump);
            CHECK((high - low) / (2.0 * bump) == 0.0);
            CHECK((high - 2.0 + low) / (bump * bump) == 0.0);
        }
    }
    // A genuine local peak inside the layer's range retains its spline derivatives.
    const std::array values{0.0, 1.0, 1.0, 2.0};
    CHECK_THAT(grid.interpolate(values, 1.5), Catch::Matchers::WithinAbs(1.0 + 21.0 / 368.0, 1e-14));
    CHECK_THAT(grid.delta(values, 1.5), Catch::Matchers::WithinAbs(-19.0 / 184.0, 1e-14));
    CHECK_THAT(grid.gamma(values, 1.5), Catch::Matchers::WithinAbs(-21.0 / 46.0, 1e-14));
    const std::array sloped{0.0, 1.0, 1.01, 1.02};
    for (const double weight : {0.001, 0.25, 0.5, 0.75, 0.999}) {
        const double spot = 1.0 + weight;
        CHECK_THAT(grid.interpolate(sloped, spot), Catch::Matchers::WithinAbs(std::lerp(1.0, 1.01, weight), 1e-14));
        CHECK_THAT(grid.delta(sloped, spot), Catch::Matchers::WithinAbs(0.01, 1e-14));
        CHECK(grid.gamma(sloped, spot) == 0.0);
    }
}

TEST_CASE("Nonuniform diffusion preserves polynomial generators and positive off diagonals", "[autocallable-accuracy]")
{
    using namespace kiyosi::detail;
    const SpatialGrid grid{4.0, 1.0, 3, 0.0, {0.0, 1.0, 2.0, 4.0}};
    for (const auto& parameters : {DiffusionParameters{0.04, 0.01, 0.2, 0.5, 0.0, &grid}, {0.1, 0.0, 0.01, 0.5, 0.0, &grid}, {0.0, 0.1, 0.01, 0.5, 0.0, &grid}}) {
        for (std::size_t index = 1; index + 1 < grid.size(); ++index) {
            const auto [a, b, c] = diffusion_coefficients(static_cast<double>(index), parameters);
            const double low = grid.asset_at(index - 1), spot = grid.asset_at(index), high = grid.asset_at(index + 1);
            CHECK(a >= 0.0);
            CHECK(c >= 0.0);
            CHECK_THAT(a + b + c, Catch::Matchers::WithinAbs(-parameters.rate, 1e-14));
            CHECK_THAT(a * low + b * spot + c * high, Catch::Matchers::WithinAbs(-parameters.dividend * spot, 1e-14));
            if (parameters.volatility == 0.2) {
                const double quadratic = (parameters.volatility * parameters.volatility + parameters.rate - 2.0 * parameters.dividend) * spot * spot;
                CHECK_THAT(a * low * low + b * spot * spot + c * high * high, Catch::Matchers::WithinAbs(quadratic, 1e-14));
            }
        }
    }
}

TEST_CASE("Nonuniform diffusion theta steps preserve affine cash and asset portfolios", "[autocallable-accuracy]")
{
    using namespace kiyosi::detail;
    const SpatialGrid grid{4.0, 1.0, 4, 0.0, {0.0, 0.4, 1.0, 2.0, 4.0}};
    constexpr double rate = 0.04, dividend = 0.01, dt = 0.01;
    const std::vector<double> old{2.0, 3.2, 5.0, 8.0, 14.0};
    std::vector<double> next(old.size());
    for (const double theta : {0.0, 0.5, 1.0}) {
        CAPTURE(theta);
        const auto decay = [=](double yield) { return (1.0 - (1.0 - theta) * yield * dt) / (1.0 + theta * yield * dt); };
        const auto expected = [&](double spot) { return 2.0 * decay(rate) + 3.0 * spot * decay(dividend); };
        FiniteDifferenceStep step{old.size()};
        REQUIRE(step.advance(old, next, dt, {rate, dividend, 0.2, theta, 0.0, &grid}, expected(0.0), expected(4.0)));
        for (std::size_t index = 0; index < next.size(); ++index)
            CHECK_THAT(next[index], Catch::Matchers::WithinAbs(expected(grid.asset_at(index)), 1e-13));
    }
}

TEST_CASE("Finite-difference speed validates shifted prices before using gamma", "[fd-speed]")
{
    using namespace kiyosi;
    std::vector<double> prices;
    std::vector<double> gammas;
    bool reject_low = false;
    const auto price_at = [&](double spot) -> Result<double> {
        prices.push_back(spot);
        if (reject_low && spot < 100.0)
            return std::unexpected(Error{ErrorCategory::invalid_result, "shifted price rejected"});
        return spot;
    };
    const auto gamma_at = [&](double spot) {
        gammas.push_back(spot);
        return 3.0 * spot;
    };
    SECTION("Available stencil uses both shifted gammas")
    {
        const auto speed = detail::finite_difference_speed(100.0, 1.0, 200.0, price_at, gamma_at);
        REQUIRE(speed);
        CHECK(*speed == std::optional{3.0});
        CHECK(prices == std::vector<double>{101.0, 99.0});
        CHECK(gammas.size() == 2);
    }
    SECTION("Unavailable stencils perform no shifted valuations")
    {
        for (const auto [spot, h, upper] : {std::array{100.0, 0.0, 200.0},
                                            std::array{1.0, 1.0, 200.0},
                                            std::array{100.0, 1.0, 101.0},
                                            std::array{100.0, std::numeric_limits<double>::quiet_NaN(), 200.0}}) {
            const auto speed = detail::finite_difference_speed(spot, h, upper, price_at, gamma_at);
            REQUIRE(speed);
            CHECK_FALSE(speed->has_value());
        }
        CHECK(prices.empty());
        CHECK(gammas.empty());
    }
    SECTION("A failed feasible price prevents gamma evaluation")
    {
        reject_low = true;
        const auto speed = detail::finite_difference_speed(100.0, 1.0, 200.0, price_at, gamma_at);
        REQUIRE_FALSE(speed);
        CHECK(speed.error().category == ErrorCategory::invalid_result);
        CHECK(prices == std::vector<double>{101.0, 99.0});
        CHECK(gammas.empty());
    }
}

TEST_CASE("Finite-difference results select measures and preserve shifted failures", "[fd-result]")
{
    using namespace kiyosi;
    int price_calls = 0, delta_calls = 0, gamma_calls = 0;
    bool reject_price = false;
    const auto price_at = [&](double spot) -> Result<double> {
        ++price_calls;
        if (reject_price)
            return std::unexpected(Error{ErrorCategory::invalid_result, "shifted price rejected"});
        return spot;
    };
    const auto delta_at = [&](double) { ++delta_calls; return 7.0; };
    const auto gamma_at = [&](double spot) { ++gamma_calls; return 3.0 * spot; };
    SECTION("Only requested derivatives are evaluated")
    {
        for (const auto request : {GreeksRequest{}, GreeksRequest{Greek::delta}, GreeksRequest{Greek::gamma},
                                   GreeksRequest{Greek::speed}, GreeksRequest{Greek::delta, Greek::gamma, Greek::speed}}) {
            price_calls = delta_calls = gamma_calls = 0;
            const auto result = detail::finite_difference_result(
                123.0, 100.0, 200.0, request, 1.0, price_at, delta_at, gamma_at);
            REQUIRE(result);
            CHECK(result->price() == 123.0);
            for (const auto [greek, expected] : {std::pair{Greek::delta, 7.0}, std::pair{Greek::gamma, 300.0},
                                                 std::pair{Greek::speed, 3.0}}) {
                CHECK(result->has(greek) == request.has(greek));
                if (request.has(greek)) CHECK(result->require(greek) == Result<double>{expected});
            }
            CHECK(price_calls == (request.has(Greek::speed) ? 2 : 0));
            CHECK(delta_calls == (request.has(Greek::delta) ? 1 : 0));
            CHECK(gamma_calls == (request.has(Greek::gamma) ? 1 : 0) + (request.has(Greek::speed) ? 2 : 0));
        }
    }
    SECTION("Unavailable speed leaves the price without shifted evaluations")
    {
        const auto result = detail::finite_difference_result(
            123.0, 100.0, 100.5, {Greek::speed}, 1.0, price_at, delta_at, gamma_at);
        REQUIRE(result);
        CHECK(result->price() == 123.0);
        CHECK_FALSE(result->has(Greek::speed));
        CHECK(price_calls == 0);
        CHECK(gamma_calls == 0);
    }
    SECTION("A shifted price error stops derivative evaluation")
    {
        reject_price = true;
        const auto result = detail::finite_difference_result(
            123.0, 100.0, 200.0, true, 1.0, price_at, delta_at, gamma_at);
        REQUIRE_FALSE(result);
        CHECK(result.error().category == ErrorCategory::invalid_result);
        CHECK(result.error().message == "shifted price rejected");
        CHECK(price_calls == 1);
        CHECK(delta_calls == 0);
        CHECK(gamma_calls == 0);
    }
}

TEST_CASE("Stretched finite-difference grids preserve asset derivative units", "[autocallable-domain]")
{
    using namespace kiyosi;
    const auto grid = detail::make_spatial_grid({800, 200}, 1e6, {100.0}, 100.0);
    REQUIRE(grid);
    std::vector<double> values(grid->size());
    for (std::size_t index = 0; index < values.size(); ++index)
        values[index] = grid->asset_at(index);
    CHECK(values.front() == 0.0);
    CHECK(values.back() == 1e6);
    for (const double spot : {50.0, 100.0, 200.0}) {
        CAPTURE(spot);
        CHECK_THAT(grid->interpolate(values, spot), Catch::Matchers::WithinAbs(spot, 1e-5));
        CHECK_THAT(grid->delta(values, spot), Catch::Matchers::WithinAbs(1.0, 0.0001));
        CHECK_THAT(grid->gamma(values, spot), Catch::Matchers::WithinAbs(0.0, 1e-6));
    }
}

TEST_CASE("Finite-difference spline interpolation preserves nodes and affine values")
{
    for (const int steps : {3, 8}) {
        const kiyosi::detail::SpatialGrid grid{static_cast<double>(steps), 1.0, steps};
        std::vector<double> values(grid.size());
        for (std::size_t index = 0; index < values.size(); ++index)
            values[index] = 2.0 * static_cast<double>(index) - 3.0;
        for (int index = 0; index <= 4 * steps; ++index) {
            const double spot = static_cast<double>(index) / 4.0;
            CHECK(std::abs(grid.interpolate(values, spot) - (2.0 * spot - 3.0)) < 1e-12);
        }
        values[1] += 0.5;
        for (int index = 0; index <= steps; ++index)
            CHECK(grid.interpolate(values, static_cast<double>(index)) == values[index]);
    }
}

TEST_CASE("Finite-difference interpolation returns exact nodes without snapping nearby spots", "[fd-performance]")
{
    const kiyosi::detail::SpatialGrid grid{4.0, 1.0, 4};
    const double large = std::numeric_limits<double>::max();
    const std::array extreme{large, -large, large, -large, large};
    for (std::size_t index = 0; index < extreme.size(); ++index)
        CHECK(grid.interpolate(extreme, static_cast<double>(index)) == extreme[index]);

    const std::array affine{-3.0, -1.0, 1.0, 3.0, 5.0};
    for (const double spot : {std::nextafter(1.0, 0.0), std::nextafter(1.0, 2.0)}) {
        CHECK(grid.interpolate(affine, spot) == 2.0 * spot - 3.0);
        CHECK(grid.interpolate(affine, spot) != affine[1]);
    }
    const std::array curved{0.0, 1.0, 0.0, 1.0, 0.0};
    CHECK(std::abs(grid.interpolate(curved, 0.5) - 43.0 / 56.0) < 1e-15);
    CHECK(std::abs(grid.interpolate(curved, 1.5) - 25.0 / 56.0) < 1e-15);
}

TEST_CASE("Finite-difference spline curvature supports repeated lookups", "[fd-performance]")
{
    const kiyosi::detail::SpatialGrid grid{4.0, 1.0, 4};
    const std::array curved{0.0, 1.0, 0.0, 1.0, 0.0};
    std::vector<double> curvature;
    CHECK(grid.interpolate(curved, 1.0, curvature) == 1.0);
    CHECK(curvature.empty());
    for (const double spot : {0.5, 1.5, 3.5, 0.5}) {
        CAPTURE(spot);
        const double expected = spot == 1.5 ? 25.0 / 56.0 : 43.0 / 56.0;
        CHECK(std::abs(grid.interpolate(curved, spot, curvature) - expected) < 1e-15);
    }
    for (std::size_t index = 0; index < curved.size(); ++index)
        CHECK(grid.interpolate(curved, static_cast<double>(index), curvature) == curved[index]);
}

TEST_CASE("Finite-difference spline interpolation stays within neighboring payoff values", "[audit-fixes]")
{
    const kiyosi::detail::SpatialGrid grid{8.0, 1.0, 8};
    for (const double sign : {1.0, -1.0}) {
        std::vector<double> values(grid.size());
        for (std::size_t index = 0; index < values.size(); ++index)
            values[index] = std::max(sign * (static_cast<double>(index) - 4.0), 0.0);
        std::vector<double> curvature;
        for (int sample = 0; sample <= 32; ++sample) {
            const double spot = static_cast<double>(sample) / 4.0;
            const auto index = std::min(static_cast<std::size_t>(spot), values.size() - 2);
            const double value = grid.interpolate(values, spot);
            CHECK(value >= std::min(values[index], values[index + 1]));
            CHECK(value <= std::max(values[index], values[index + 1]));
            CHECK(grid.interpolate(values, spot, curvature) == value);
        }
        CHECK(grid.interpolate(values, sign > 0.0 ? 3.75 : 4.25) == 0.0);
    }
}

TEST_CASE("Linear boundary steppers refresh parameters without retaining old operators", "[fd-implied-workspace]")
{
    using namespace kiyosi::detail;
    const std::vector<double> first{1.0, 1.2, 1.5, 2.1, 3.0}, second{2.0, 1.5, 1.0, 0.5, 0.0};
    std::vector<double> actual(first.size()), expected(first.size()), actual_second(first.size()), expected_second(first.size());
    LinearBoundaryStepper reused(first.size(), 4.0, 1.0, {0.04, 0.01, 0.2, 0.5});
    for (const auto parameters : {DiffusionParameters{0.04, 0.01, 0.2, 0.5}, {0.1, -0.02, 0.7, 1.0}, {0.04, 0.01, 0.2, 0.5}}) {
        reused.set_parameters(parameters);
        LinearBoundaryStepper fresh(first.size(), 4.0, 1.0, parameters);
        REQUIRE(reused.advance(first, actual, 0.01));
        REQUIRE(fresh.advance(first, expected, 0.01));
        CHECK(actual == expected);
        REQUIRE(reused.advance_pair(first, actual, second, actual_second, 0.01));
        REQUIRE(fresh.advance_pair(first, expected, second, expected_second, 0.01));
        CHECK(actual == expected);
        CHECK(actual_second == expected_second);
    }
}

TEST_CASE("Finite-difference grids reject zero spacing while preserving small positive spacing", "[audit-fixes]")
{
    const double tiny = std::numeric_limits<double>::denorm_min();
    const auto invalid = kiyosi::detail::make_spatial_grid({}, 4.0 * tiny, {tiny});
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_parameter);
    const kiyosi::FiniteDifferenceSettings settings;
    for (const double upper : {2.0 * settings.asset_step_count * tiny, 400.0}) {
        const auto valid = kiyosi::detail::make_spatial_grid({}, upper, {tiny});
        REQUIRE(valid);
        CHECK(valid->spacing == upper / settings.asset_step_count);
        CHECK(valid->spacing > 0.0);
        CHECK(std::isfinite(valid->spacing));
    }
}

TEST_CASE("Uniform finite-difference intervals share one exact width", "[fd-performance]")
{
    for (const double maturity : {1.0 / (365.0 * 86400.0 * 1e6), 91.0 / 365.0, 1.0, 2.75}) {
        for (const int steps : {1, 3, 80, 200, 1000, 100'000}) {
            CAPTURE(maturity, steps);
            const auto grid = kiyosi::detail::make_finite_difference_time_grid(maturity, steps);
            REQUIRE(grid.time_steps.size() == static_cast<std::size_t>(steps));
            CHECK(std::ranges::all_of(grid.time_steps, [=](double dt) { return dt == maturity / steps; }));
            CHECK(grid.times.front() == 0.0);
            CHECK(grid.times.back() == maturity);
            const auto event_grid = kiyosi::detail::make_finite_difference_time_grid(maturity, steps, {0.0, maturity});
            CHECK(grid.times == event_grid.times);
            CHECK(grid.time_steps == event_grid.time_steps);
        }
    }
}

TEST_CASE("Degenerate finite-difference time grids retain unique nodes", "[fd-performance]")
{
    for (const double maturity : {0.0, 4.0 * std::numeric_limits<double>::denorm_min()}) {
        const auto grid = kiyosi::detail::make_finite_difference_time_grid(maturity, 8);
        const auto event_grid = kiyosi::detail::make_finite_difference_time_grid(maturity, 8, {0.0, maturity});
        CHECK(grid.times == event_grid.times);
        CHECK(grid.time_steps == event_grid.time_steps);
        CHECK(std::ranges::adjacent_find(grid.times) == grid.times.end());
    }
}

TEST_CASE("Finite-difference event intervals retain exact anchors and split widths", "[fd-performance]")
{
    const double maturity = 91.0 / 365.0;
    const double middle = maturity / 2.0;
    const double before = std::nextafter(middle, 0.0);
    const double after = std::nextafter(middle, maturity);
    const auto grid = kiyosi::detail::make_finite_difference_time_grid(
        maturity, 4, {after, middle, before, before, 0.0, maturity});
    const std::vector expected_times{0.0, maturity / 4.0, before, middle, after, maturity * 3.0 / 4.0, maturity};
    const std::vector expected_steps{maturity / 4.0, before - maturity / 4.0, middle - before,
                                     after - middle, maturity * 3.0 / 4.0 - after, maturity / 4.0};
    CHECK(grid.times == expected_times);
    CHECK(grid.time_steps == expected_steps);
    CHECK(std::ranges::all_of(grid.time_steps, [](double dt) { return dt > 0.0; }));
    const auto stable = kiyosi::detail::check_explicit_stability(
        kiyosi::FiniteDifferenceScheme::explicit_euler, grid, 0.2, 0.04, 0.01, 3);
    REQUIRE(stable);
    const auto unstable = kiyosi::detail::check_explicit_stability(
        kiyosi::FiniteDifferenceScheme::explicit_euler, grid, 0.2, 0.04, 0.01, 200);
    REQUIRE_FALSE(unstable);
    CHECK(unstable.error().category == kiyosi::ErrorCategory::invalid_parameter);
}

TEST_CASE("Daily finite-difference anchors replace rounded uniform duplicates", "[fd-performance]")
{
    std::vector<double> events;
    for (int day = 0; day <= 366; ++day)
        events.push_back(day / 365.0);
    const auto grid = kiyosi::detail::make_finite_difference_time_grid(366.0 / 365.0, 366, events);
    CHECK(grid.times == events);
    REQUIRE(grid.time_steps.size() == 366);
    for (const double dt : grid.time_steps)
        CHECK_THAT(dt, Catch::Matchers::WithinAbs(1.0 / 365.0, 3e-16));
}

TEST_CASE("Calendar finite-difference intervals share exact daily widths", "[fd-performance]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2024} / 1 / 1}, end{std::chrono::year{2025} / 1 / 1};
    std::vector<Date> events;
    std::vector<double> expected;
    for (int day = 0; day <= 366; ++day) {
        events.push_back(start + std::chrono::days{day});
        expected.push_back(day / 365.0);
    }
    std::ranges::reverse(events);
    events.push_back(start + std::chrono::days{1});
    for (const int steps : {1, 366}) {
        const auto grid = detail::make_finite_difference_time_grid(start_of_day(start), end, steps, events);
        CHECK(grid.times == expected);
        REQUIRE(grid.time_steps.size() == 366);
        CHECK(std::ranges::all_of(grid.time_steps, [](double dt) { return dt == 1.0 / 365.0; }));
    }
    const auto valuation = start_of_day(start) + std::chrono::hours{12} + std::chrono::microseconds{1};
    const auto intraday = detail::make_finite_difference_time_grid(valuation, end, 1, events);
    CHECK(intraday.time_steps.front() == detail::actual_365_fixed_year_fraction(valuation, start_of_day(start + std::chrono::days{1})));
    CHECK(std::ranges::all_of(intraday.time_steps | std::views::drop(1), [](double dt) { return dt == 1.0 / 365.0; }));
    const auto just_before = detail::make_finite_difference_time_grid(
        start_of_day(end) - std::chrono::microseconds{1}, end, 1, {end});
    REQUIRE(just_before.time_steps.size() == 1);
    CHECK(just_before.time_steps.front() == 1.0 / 31'536'000'000'000.0);

    const auto mixed = detail::make_finite_difference_time_grid(start_of_day(start), start + std::chrono::days{8}, 2,
                                                                {start + std::chrono::days{1}, start + std::chrono::days{2}, start + std::chrono::days{7}});
    const std::vector mixed_times{0.0, 1.0 / 365.0, 2.0 / 365.0, 4.0 / 365.0, 7.0 / 365.0, 8.0 / 365.0};
    CHECK(mixed.times == mixed_times);
    CHECK(mixed.time_steps.front() == 1.0 / 365.0);
    CHECK(mixed.time_steps.back() == 1.0 / 365.0);
    CHECK(mixed.time_steps[2] == mixed_times[3] - mixed_times[2]);
    CHECK(mixed.time_steps[3] == mixed_times[4] - mixed_times[3]);
}

TEST_CASE("Calendar finite-difference prices retain daily reference values", "[fd-performance]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2024} / 1 / 1}, end{std::chrono::year{2025} / 1 / 1};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25, start, all_days_calendar());
    const auto check = [&](const auto& option, const auto make_engine, const std::array<std::array<double, 3>, 3>& expected) {
        std::size_t index = 0;
        for (const auto scheme : {FiniteDifferenceScheme::crank_nicolson, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::explicit_euler}) {
            const auto engine = make_engine({.asset_step_count = 80, .time_step_count = 1, .scheme = scheme, .asset_upper_boundary = 400.0}).value();
            const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
            REQUIRE(result);
            REQUIRE(result->has(Greek::delta));
            REQUIRE(result->has(Greek::gamma));
            CHECK(engine.price(option, context) == result->price());
            CHECK(std::abs(result->price() - expected[index][0]) < 1e-9);
            CHECK(std::abs(**result->get(Greek::delta) - expected[index][1]) < 1e-9);
            CHECK(std::abs(**result->get(Greek::gamma) - expected[index][2]) < 1e-9);
            ++index;
        }
    };
    const auto accumulator = *make_accumulator({.strike = 100.0, .knock_out_level = 115.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = end});
    check(accumulator, make_finite_difference_accumulator_engine, {{{-2125.177940477314, 314.25537377676187, -24.53113696074924}, {-2120.221666260853, 314.4815991820858, -24.573439894742528}, {-2130.1933395583947, 314.0331999420339, -24.487720823425548}}});
    std::vector<Date> observations;
    for (int day = 1; day <= 366; ++day)
        observations.push_back(start + std::chrono::days{day});
    const auto barrier = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 110.0, .barrier_type = BarrierType::up_and_out, .rebate = 2.0, .observation_mode = ObservationMode::scheduled, .observation_dates = observations});
    check(barrier, make_finite_difference_barrier_engine, {{{1.349334949897279, 0.05874014816127894, 0.00011939130239541559}, {1.3468904122711003, 0.058637636418814774, 0.00012654044204111787}, {1.3518636988741737, 0.0588457671037437, 0.00011178373357470883}}});
}

TEST_CASE("Finite-difference merging preserves adjacent events and later uniform widths", "[fd-performance]")
{
    const double middle = 0.5;
    const double before = std::nextafter(middle, 0.0), after = std::nextafter(middle, 1.0);
    const auto adjacent = kiyosi::detail::make_finite_difference_time_grid(1.0, 4, {after, before, before});
    CHECK(adjacent.times == std::vector<double>{0.0, 0.25, before, after, 0.75, 1.0});
    CHECK(adjacent.time_steps == std::vector<double>{0.25, before - 0.25, after - before, 0.75 - after, 0.25});

    const double event = std::nextafter(1.0 / 3.0, 1.0);
    const auto later = kiyosi::detail::make_finite_difference_time_grid(1.0, 3, {event});
    CHECK(later.times == std::vector<double>{0.0, event, 2.0 / 3.0, 1.0});
    CHECK(later.time_steps == std::vector<double>{event, 2.0 / 3.0 - event, 1.0 / 3.0});
}

TEST_CASE("Implicit Euler skips overflowing zero-weight terms", "[fd-performance]")
{
    using namespace kiyosi::detail;
    constexpr double value = 1e308;
    const DiffusionParameters parameters{0.0, 0.0, 2.0, 1.0};
    std::vector<double> old(5, value), next(5);
    FiniteDifferenceStep step{old.size()};
    for (int iteration = 0; iteration < 2; ++iteration) {
        REQUIRE(step.advance(old, next, 0.01, parameters, value, value));
        for (const double actual : next)
            CHECK_THAT(actual, Catch::Matchers::WithinRel(value, 1e-14));
        old.swap(next);
    }
}

TEST_CASE("Paired finite-difference advances match independent layers")
{
    const auto size = GENERATE(std::size_t{4}, std::size_t{7});
    const auto volatility = GENERATE(0.2, 0.001);
    const auto sinh_spacing = GENERATE(0.0, 0.2);
    constexpr double upper = 240.0;
    const double spacing = upper / static_cast<double>(size - 1);
    const std::array time_step_count{0.17, 0.17, 0.03, 0.17, 0.03, 0.11, 0.11, 0.03, 0.17};

    for (const double theta : {0.0, 1.0, 0.5}) {
        CAPTURE(theta);
        const kiyosi::detail::DiffusionParameters parameters{
            .rate = 0.03, .dividend = 0.01, .volatility = volatility, .theta = theta, .sinh_spacing = sinh_spacing};
        kiyosi::detail::LinearBoundaryStepper paired{size, upper, spacing, parameters};
        kiyosi::detail::LinearBoundaryStepper first_independent{size, upper, spacing, parameters};
        kiyosi::detail::LinearBoundaryStepper second_independent{size, upper, spacing, parameters};
        std::vector<double> first{0.2, 0.8, 1.7, 3.1, 5.2, 8.0, 11.5};
        std::vector<double> second{-3.0, -2.2, -0.7, 1.6, 4.8, 9.0, 14.3};
        first.resize(size);
        second.resize(size);
        auto expected_first = first;
        auto expected_second = second;
        std::vector<double> next_first(size), next_second(size);
        std::vector<double> next_expected_first(size), next_expected_second(size);

        for (const double dt : time_step_count) {
            REQUIRE(paired.advance_pair(first, next_first, second, next_second, dt));
            REQUIRE(first_independent.advance(expected_first, next_expected_first, dt));
            REQUIRE(second_independent.advance(expected_second, next_expected_second, dt));
            CHECK(next_first == next_expected_first);
            CHECK(next_second == next_expected_second);
            first.swap(next_first);
            second.swap(next_second);
            expected_first.swap(next_expected_first);
            expected_second.swap(next_expected_second);
        }
    }
}

TEST_CASE("Implicit finite-difference steps preserve finiteness failure guarantees")
{
    constexpr std::size_t size = 5;
    constexpr double sentinel = -123.0;
    const double infinity = std::numeric_limits<double>::infinity();

    SECTION("invalid single-layer interiors are rejected before copying")
    {
        kiyosi::detail::FiniteDifferenceStep step{size};
        const std::vector<double> old{1.0, 2.0, infinity, 4.0, 5.0};
        std::vector<double> next(size, sentinel);

        CHECK_FALSE(step.advance(old, next, 0.1, {0.03, 0.01, 0.2, 1.0}, 0.0, 10.0));
        CHECK(next[1] == sentinel);
        CHECK(next[2] == sentinel);
        CHECK(next[3] == sentinel);
    }

    SECTION("an invalid paired interior prevents either layer from being copied")
    {
        kiyosi::detail::FiniteDifferenceStep step{size};
        const std::vector<double> first_old{1.0, 2.0, 3.0, 4.0, 5.0};
        const std::vector<double> second_old{1.0, 2.0, infinity, 4.0, 5.0};
        std::vector<double> first_next(size, sentinel);
        std::vector<double> second_next(size, sentinel);
        const kiyosi::detail::DiffusionParameters parameters{
            .rate = 0.03, .dividend = 0.01, .volatility = 0.2, .theta = 1.0};

        CHECK_FALSE(step.advance_pair(first_old, first_next, second_old, second_next, 0.1,
                                      parameters, {0.0, 10.0}, {0.0, 10.0}));
        for (std::size_t index = 1; index + 1 < size; ++index) {
            CHECK(first_next[index] == sentinel);
            CHECK(second_next[index] == sentinel);
        }
    }

    SECTION("invalid boundaries are rejected after finite interiors are copied")
    {
        const auto check_boundaries = [&](double lower, double upper) {
            kiyosi::detail::FiniteDifferenceStep step{size};
            const std::vector<double> old(size, 1.0);
            std::vector<double> next(size, sentinel);

            CHECK_FALSE(step.advance(
                old, next, 0.1, {0.03, 0.01, 0.2, 1.0}, lower, upper,
                [](int) -> std::optional<double> { return 7.0; }));
            CHECK(next[1] == 7.0);
            CHECK(next[2] == 7.0);
            CHECK(next[3] == 7.0);
        };

        check_boundaries(infinity, 10.0);
        check_boundaries(0.0, infinity);
    }
}

TEST_CASE("Stretched finite-difference coefficients follow parameter changes", "[audit-fixes]")
{
    kiyosi::detail::FiniteDifferenceStep reused{5};
    const std::vector<double> old{0.0, 1.0, 4.0, 9.0, 16.0};
    for (const double volatility : {0.2, 0.4, 0.2}) {
        const kiyosi::detail::DiffusionParameters parameters{
            .rate = 0.03, .dividend = 0.01, .volatility = volatility, .theta = 0.5, .sinh_spacing = 0.1};
        kiyosi::detail::FiniteDifferenceStep fresh{5};
        std::vector<double> actual(5), expected(5);
        const auto unconstrained = [](int) -> std::optional<double> { return std::nullopt; };
        REQUIRE(reused.advance(old, actual, 0.1, parameters, 0.0, 16.0, unconstrained));
        REQUIRE(fresh.advance(old, expected, 0.1, parameters, 0.0, 16.0, unconstrained));
        CHECK(actual == expected);
    }
}

TEST_CASE("Uniform finite-difference coefficients preserve quadratic evolution", "[fd-factor-cache]")
{
    kiyosi::detail::FiniteDifferenceStep reused{5};
    const std::vector<double> old{0.0, 1.0, 4.0, 9.0, 16.0};
    for (const double volatility : {0.2, 0.2, 0.4, 0.2}) {
        CAPTURE(volatility);
        const kiyosi::detail::DiffusionParameters parameters{0.03, 0.01, volatility, 0.0};
        // The Black-Scholes operator maps S^2 to (rate - 2 * dividend + volatility^2) * S^2.
        const double growth = 1.0 + 0.01 * (0.03 - 2.0 * 0.01 + volatility * volatility);
        std::vector<double> actual(5);
        REQUIRE(reused.advance(old, actual, 0.01, parameters, 0.0, 16.0 * growth));
        for (std::size_t index = 0; index < old.size(); ++index)
            CHECK(std::abs(actual[index] - old[index] * growth) < 1e-12);
    }
}

TEST_CASE("Finite-difference factors follow time steps parameters and constraint masks", "[fd-factor-cache]")
{
    using namespace kiyosi::detail;
    FiniteDifferenceStep reused{7};
    const std::vector<double> old{0.0, 1.0, 4.0, 9.0, 16.0, 25.0, 36.0};
    for (const double spacing : {0.0, 0.1, 0.0})
        for (const double theta : {1.0, 0.5, 0.0, 0.5})
            for (const double rate : {0.03, -0.01})
                for (const double dividend : {0.01, 0.02})
                    for (const double volatility : {0.2, 0.4})
                        for (const double dt : {0.1, 0.1, 0.05, 0.1, 0.05, 0.025, 0.1, 0.025, 0.05})
                            for (const int pin : {0, 2, 2, 3, 0, 5, 5, 0})
                                for (const double value : {7.0, 8.0}) {
                                    CAPTURE(spacing, theta, rate, dividend, volatility, dt, pin, value);
                                    const DiffusionParameters parameters{rate, dividend, volatility, theta, spacing};
                                    const auto constraint = [=](int index) -> std::optional<double> {
                                        return index == pin ? std::optional{value} : std::nullopt;
                                    };
                                    FiniteDifferenceStep fresh{7};
                                    std::vector<double> actual(7), expected(7);
                                    REQUIRE(reused.advance(old, actual, dt, parameters, 0.0, 36.0, constraint));
                                    REQUIRE(fresh.advance(old, expected, dt, parameters, 0.0, 36.0, constraint));
                                    CHECK(actual == expected);
                                    if (pin != 0) CHECK(actual[static_cast<std::size_t>(pin)] == value);
                                }
}

TEST_CASE("Finite-difference factors follow constrained and unconstrained solve transitions", "[fd-factor-cache]")
{
    using namespace kiyosi::detail;
    const std::vector<double> first{0.0, 1.0, 4.0, 9.0, 16.0, 25.0, 36.0};
    const std::vector<double> second{7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0};
    for (const double spacing : {0.0, 0.1})
        for (const double theta : {0.0, 0.5, 1.0}) {
            FiniteDifferenceStep reused{first.size()};
            const DiffusionParameters parameters{0.04, 0.01, 0.2, theta, spacing};
            for (const double dt : {0.1, 0.05, 0.025, 0.1, 0.05, 0.025})
                for (const bool constrained : {true, false, false, true}) {
                    CAPTURE(spacing, theta, dt, constrained);
                    const auto constraint = [=](int index) -> std::optional<double> {
                        return constrained && index >= 4 ? std::optional{8.0} : std::nullopt;
                    };
                    std::vector<double> actual(first.size()), expected(first.size());
                    FiniteDifferenceStep reference{first.size()};
                    REQUIRE(reference.advance(first, expected, dt, parameters, 0.0, 36.0, constraint));
                    if (constrained)
                        REQUIRE(reused.advance(first, actual, dt, parameters, 0.0, 36.0, constraint));
                    else
                        REQUIRE(reused.advance(first, actual, dt, parameters, 0.0, 36.0));
                    CHECK(actual == expected);

                    // Pairing must discard a previously pinned region in either layer.
                    std::vector<double> actual_second(second.size()), expected_second(second.size());
                    FiniteDifferenceStep first_reference{first.size()}, second_reference{second.size()};
                    const auto free_node = [](int) -> std::optional<double> { return std::nullopt; };
                    REQUIRE(first_reference.advance(first, expected, dt, parameters, 0.0, 36.0, free_node));
                    REQUIRE(second_reference.advance(second, expected_second, dt, parameters, 7.0, 1.0, free_node));
                    REQUIRE(reused.advance_pair(first, actual, second, actual_second, dt, parameters, {0.0, 36.0}, {7.0, 1.0}));
                    CHECK(actual == expected);
                    CHECK(actual_second == expected_second);
                    if (constrained) {
                        REQUIRE(reused.advance(first, actual, dt, parameters, 0.0, 36.0, constraint));
                        REQUIRE(reused.advance(first, actual, dt, parameters, 0.0, 36.0));
                        CHECK(actual == expected);
                    }
                }
        }
}

TEST_CASE("Finite-difference factor cache recovers after failed solves", "[fd-factor-cache]")
{
    using namespace kiyosi::detail;
    FiniteDifferenceStep reused{5};
    const std::vector<double> old{0.0, 1.0, 4.0, 9.0, 16.0};
    std::vector<double> actual(5), expected(5);
    const DiffusionParameters valid{0.03, 0.01, 0.2, 0.5};
    REQUIRE(reused.advance(old, actual, 0.1, valid, 0.0, 16.0));
    REQUIRE(reused.advance(old, actual, 0.05, valid, 0.0, 16.0));
    REQUIRE(reused.advance(old, actual, 0.025, valid, 0.0, 16.0));
    for (int attempt = 0; attempt < 3; ++attempt) {
        // Binary-exact inputs keep the singular pivot zero with or without fused arithmetic.
        CHECK_FALSE(reused.advance(old, actual, 0.125, {-8.0, -8.0, 0.0, 1.0}, 0.0, 16.0));
        auto invalid_old = old;
        invalid_old[2] = std::numeric_limits<double>::infinity();
        CHECK_FALSE(reused.advance(invalid_old, actual, 0.1, valid, 0.0, 16.0));
        for (const double dt : {0.1, 0.05, 0.025, 0.05, 0.1, 0.025}) {
            FiniteDifferenceStep fresh{5};
            REQUIRE(reused.advance(old, actual, dt, valid, 0.0, 16.0));
            REQUIRE(fresh.advance(old, expected, dt, valid, 0.0, 16.0));
            CHECK(actual == expected);
        }
    }
}

TEST_CASE("Finite-difference fixed edges preserve the full system equations", "[fd-factor-cache]")
{
    using namespace kiyosi::detail;
    const std::vector<double> old{0.0, 1.0, 4.0, 9.0, 16.0, 25.0, 36.0};
    for (const double theta : {0.0, 0.5, 1.0}) {
        FiniteDifferenceStep step{old.size()};
        for (const int mask : {0, 3, 3, 24, 27, 31, 30, 15, 21, 0})
            for (const double pin : {7.0, 8.0}) {
                CAPTURE(theta, mask, pin);
                std::vector<double> next(old.size());
                REQUIRE(step.advance(old, next, 0.1, {0.04, 0.01, 0.2, theta}, 0.0, 50.0,
                                     [=](int index) -> std::optional<double> {
                                         return mask & (1 << (index - 1)) ? std::optional{pin} : std::nullopt;
                                     }));
                CHECK(next.front() == 0.0);
                CHECK(next.back() == 50.0);
                for (std::size_t index = 1; index + 1 < old.size(); ++index) {
                    if (mask & (1 << (index - 1))) {
                        CHECK(next[index] == pin);
                        continue;
                    }
                    const double node = static_cast<double>(index);
                    const double a = 0.02 * node * node - 0.015 * node;
                    const double b = -0.04 * node * node - 0.04;
                    const double c = 0.02 * node * node + 0.015 * node;
                    const double lhs = next[index] - theta * 0.1 *
                                                         (a * next[index - 1] + b * next[index] + c * next[index + 1]);
                    const double rhs = old[index] + (1.0 - theta) * 0.1 *
                                                        (a * old[index - 1] + b * old[index] + c * old[index + 1]);
                    CHECK(std::abs(lhs - rhs) < 1e-12);
                }
            }
        std::vector<double> next(old.size(), -123.0);
        CHECK_FALSE(step.advance(old, next, 0.1, {0.04, 0.01, 0.2, theta}, 0.0, 50.0,
                                 [](int) -> std::optional<double> { return std::numeric_limits<double>::infinity(); }));
        for (std::size_t index = 1; index + 1 < next.size(); ++index)
            CHECK(next[index] == (theta == 0.0 ? std::numeric_limits<double>::infinity() : -123.0));
        REQUIRE(step.advance(old, next, 0.1, {0.04, 0.01, 0.2, theta}, 0.0, 50.0));
        CHECK(std::ranges::all_of(next, [](double value) { return std::isfinite(value); }));
    }
}
