#include <catch2/catch_test_macros.hpp>
#include <catch2/generators/catch_generators.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <array>
#include <cmath>
#include <limits>
#include <numbers>
#include <vector>

#include <oneapi/tbb/task_arena.h>
#include <oneapi/tbb/task_group.h>

#include "pricing/detail/quadrature.hpp"

namespace {
using kiyosi::detail::scaled_exponential;
using namespace kiyosi::detail::quadrature;

double reference_integral(const Curve& curve, const std::vector<IntervalMoments>& weights,
                          const std::array<double, 2>& tails, double log_discount)
{
    Sum sum;
    sum.add(scaled_exponential(curve.low, tails[0] + log_discount));
    sum.add(scaled_exponential(curve.high, tails[1] + log_discount));
    const double discount = std::exp(log_discount);
    for (std::size_t cell = 0; cell < weights.size(); ++cell) {
        const auto& w = weights[cell];
        const auto& p = curve.cells[cell];
        const double mean = p[0] + p[1] * w.conditional[0] + p[2] * w.conditional[1] + p[3] * w.conditional[2];
        const double term = w.log_mass < -1500.0 ? 0.0 : w.mass > 0.0 ? mean * w.mass
                                                                      : scaled_exponential(mean, w.log_mass);
        sum.add(std::isnormal(discount) ? term * discount : scaled_exponential(term, log_discount));
    }
    return sum.total();
}

} // namespace

TEST_CASE("Quadrature grid preserves exact breaks and removes duplicate nodes", "[quadrature-kernel]")
{
    const auto nodes = integration_nodes(-1.0, 1.0, 4, std::array{0.25, -0.5, 0.25});
    REQUIRE(nodes);
    CHECK(*nodes == std::vector<double>{-1.0, -0.5, 0.0, 0.25, 0.5, 1.0});
}

TEST_CASE("Quadrature grid rejects unrepresentable domains and collapsed nodes", "[quadrature-kernel]")
{
    const double nan = std::numeric_limits<double>::quiet_NaN();
    for (const auto bounds : {std::array{-infinity, 1.0}, std::array{0.0, infinity},
                              std::array{nan, 1.0}, std::array{0.0, nan},
                              std::array{1.0, 1.0}, std::array{1.0, 0.0},
                              std::array{1.0, std::nextafter(1.0, 2.0)}}) {
        CAPTURE(bounds);
        const auto nodes = integration_nodes(bounds[0], bounds[1], 4, {});
        REQUIRE_FALSE(nodes);
        CHECK(nodes.error().category == kiyosi::ErrorCategory::invalid_result);
    }
}

TEST_CASE("Quadrature broad-cell moments match symmetric Gaussian identities", "[quadrature-kernel]")
{
    const double mass = std::erf(1.0 / std::numbers::sqrt2);
    const double variance = 1.0 - 2.0 * std::exp(-0.5) / (std::sqrt(2.0 * std::numbers::pi) * mass);
    const auto moments = cell_moments(-1.0, 1.0, 0.0, 1.0);
    CHECK_THAT(moments.mass, Catch::Matchers::WithinAbs(mass, 2e-15));
    CHECK_THAT(moments.log_mass, Catch::Matchers::WithinAbs(std::log(mass), 2e-15));
    CHECK_THAT(moments.conditional[0], Catch::Matchers::WithinAbs(0.5, 2e-15));
    CHECK_THAT(moments.conditional[1], Catch::Matchers::WithinAbs((1.0 + variance) / 4.0, 2e-15));
    CHECK_THAT(moments.conditional[2], Catch::Matchers::WithinAbs((1.0 + 3.0 * variance) / 8.0, 2e-15));
}

TEST_CASE("Quadrature transitions preserve moments and tails across grid sizes", "[quadrature-kernel]")
{
    using namespace kiyosi::detail;
    const int count = GENERATE(4, 5, 6, 7, 17, 513);
    const double deviation = GENERATE(0.01, 0.2);
    std::vector<double> nodes, samples;
    for (int index = 0; index < count; ++index) {
        const double node = std::lerp(-1.0, 1.0, static_cast<double>(index) / (count - 1));
        nodes.push_back(node);
        samples.push_back(100.0 * std::expm1(node));
    }
    const auto curve = interpolate(nodes, samples, -123.0, 456.0);
    const auto transition = transition_weights(nodes, 0.0001, deviation);
    REQUIRE(transition.weights.size() == nodes.size() * curve.cells.size());
    std::vector<double> actual(nodes.size());
    integrate_curves(curve, transition, -0.001, actual);
    for (std::size_t target = 0; target < nodes.size(); ++target) {
        std::vector<IntervalMoments> reference;
        for (std::size_t cell = 0; cell + 1 < nodes.size(); ++cell) {
            reference.push_back(cell_moments(nodes[cell], nodes[cell + 1], nodes[target] + 0.0001, deviation));
            const auto& actual_moments = transition.weights[target * curve.cells.size() + cell];
            CHECK(actual_moments.mass == reference.back().mass);
            CHECK(actual_moments.log_mass == reference.back().log_mass);
            CHECK(actual_moments.conditional == reference.back().conditional);
        }
        const double mean = nodes[target] + 0.0001;
        const std::array tails{log_normal_cdf((nodes.front() - mean) / deviation), log_normal_cdf((mean - nodes.back()) / deviation)};
        const double expected = reference_integral(curve, reference, tails, -0.001);
        CAPTURE(count, deviation, target, expected);
        CHECK(actual[target] == expected);
    }
}

TEST_CASE("Quadrature paired curves exactly match independent integration", "[quadrature-kernel]")
{
    const auto nodes = integration_nodes(-1.0, 1.0, GENERATE(17, 128, 513), std::array{-0.123, 0.456}).value();
    std::vector<double> first_samples, second_samples;
    first_samples.reserve(nodes.size());
    second_samples.reserve(nodes.size());
    for (const auto node : nodes) {
        first_samples.push_back(100.0 * std::expm1(node));
        second_samples.push_back(-50.0 * std::exp(node));
    }
    const auto first = interpolate(nodes, first_samples, -123.0, 456.0);
    const auto second = interpolate(nodes, second_samples, 789.0, -321.0);
    const auto transition = transition_weights(nodes, 0.0001, 0.02);
    for (const double discount : {-750.0, -0.001, 700.0}) {
        std::vector<double> expected_first(nodes.size()), expected_second(nodes.size());
        integrate_curves(first, transition, discount, expected_first);
        integrate_curves(second, transition, discount, expected_second);
        std::vector<double> actual_first(nodes.size()), actual_second(nodes.size());
        integrate_curve_pair(first, second, transition, discount, actual_first, actual_second);
        CHECK(actual_first == expected_first);
        CHECK(actual_second == expected_second);
    }
}

TEST_CASE("Quadrature integration retains compensated cancellation in independent targets", "[quadrature-kernel]")
{
    const Curve curve{{Polynomial{1e16, 0.0, 0.0, 0.0}, Polynomial{1.0, 0.0, 0.0, 0.0}, Polynomial{-1e16, 0.0, 0.0, 0.0}}, 0.0, 0.0};
    Transition transition{std::vector<IntervalMoments>(7 * curve.cells.size()), std::vector<std::array<double, 2>>(7, {-infinity, -infinity})};
    for (std::size_t target = 0; target < transition.tails.size(); ++target)
        for (std::size_t cell = 0; cell < curve.cells.size(); ++cell) {
            const double mass = 0.5 * static_cast<double>(target + 1);
            transition.weights[target * curve.cells.size() + cell] = {mass, std::log(mass), {}};
        }
    std::array<double, 7> values{};
    integrate_curves(curve, transition, 0.0, values);
    for (std::size_t target = 0; target < values.size(); ++target)
        CHECK(values[target] == 0.5 * static_cast<double>(target + 1));
}

TEST_CASE("Quadrature integration recovers subnormal masses and isolates discarded targets", "[quadrature-kernel]")
{
    const Curve curve{{Polynomial{1e300, 0.0, 0.0, 0.0}}, 0.0, 0.0};
    const double subnormal = std::numeric_limits<double>::denorm_min();
    const std::array weights{
        IntervalMoments{0.5, std::log(0.5), {}}, IntervalMoments{subnormal, std::log(subnormal), {}},
        IntervalMoments{0.0, -900.0, {}}, IntervalMoments{0.0, -1600.0, {std::numeric_limits<double>::quiet_NaN(), 0.0, 0.0}},
        IntervalMoments{0.0, -800.0, {}}, IntervalMoments{0.0, -1500.0, {}}, IntervalMoments{0.0, -infinity, {}}};
    Transition transition{{weights.begin(), weights.end()}, std::vector<std::array<double, 2>>(weights.size(), {-infinity, -infinity})};
    std::array<double, weights.size()> values{};
    integrate_curves(curve, transition, 0.0, values);
    for (std::size_t target = 0; target < weights.size(); ++target) {
        const double expected = reference_integral(curve, {weights[target]}, transition.tails[target], 0.0);
        CAPTURE(target);
        CHECK(values[target] == expected);
    }
    CHECK(values[1] > 0.0);
    CHECK(values[2] > 0.0);
    CHECK(values[3] == 0.0);
    CHECK(values[4] > 0.0);
}

TEST_CASE("Quadrature integration preserves log-discount recovery and zero-recovery bounds", "[quadrature-kernel]")
{
    const double discount = GENERATE(0.0, -740.0, -800.0, -1500.0);
    const double coefficient = GENERATE(1.0, 1e19, 1e20, 1e300, -1e300);
    const Curve curve{{Polynomial{coefficient, 0.0, 0.0, 0.0}}, 0.0, 0.0};
    const std::array weights{IntervalMoments{1.0, 0.0, {}}, IntervalMoments{0.0, -801.0, {}},
                             IntervalMoments{0.0, -760.0, {}}, IntervalMoments{0.0, -799.0, {}}, IntervalMoments{0.0, -900.0, {}}};
    Transition transition{{weights.begin(), weights.end()}, std::vector<std::array<double, 2>>(weights.size(), {-infinity, -infinity})};
    std::array<double, weights.size()> values{};
    integrate_curves(curve, transition, discount, values);
    for (std::size_t target = 0; target < weights.size(); ++target) {
        CAPTURE(discount, coefficient, target);
        CHECK(values[target] == reference_integral(curve, {weights[target]}, transition.tails[target], discount));
    }
    if (discount == -740.0 && std::abs(coefficient) == 1e300) CHECK(std::abs(values[0]) > 1e-23);
    if (discount == 0.0 && coefficient == 1e19) CHECK(values[2] > 0.0);
}

TEST_CASE("Quadrature parallel rows preserve results under outer cancellation", "[quadrature-kernel]")
{
    const auto nodes = integration_nodes(-1.0, 1.0, 128, std::array{-0.123, 0.456}).value();
    std::vector<double> samples;
    samples.reserve(nodes.size());
    for (const auto node : nodes)
        samples.push_back(100.0 * std::expm1(node));
    const auto curve = interpolate(nodes, samples, -123.0, 456.0);
    const auto other = interpolate(nodes, samples, 789.0, -321.0);
    Transition reference;
    std::vector<double> expected(nodes.size());
    std::vector<double> expected_other(nodes.size());
    oneapi::tbb::task_arena serial{1};
    serial.execute([&] {
        reference = transition_weights(nodes, 0.0001, 0.02);
        integrate_curves(curve, reference, -0.001, expected);
        integrate_curves(other, reference, -0.001, expected_other);
    });
    Transition actual;
    std::vector<double> values(nodes.size(), std::numeric_limits<double>::quiet_NaN());
    std::vector<double> other_values(nodes.size(), std::numeric_limits<double>::quiet_NaN());
    oneapi::tbb::task_arena parallel{4};
    parallel.execute([&] {
        oneapi::tbb::task_group_context outer{oneapi::tbb::task_group_context::isolated};
        oneapi::tbb::task_group group{outer};
        group.run([&] {
            outer.cancel_group_execution();
            actual = transition_weights(nodes, 0.0001, 0.02);
            integrate_curve_pair(curve, other, actual, -0.001, values, other_values);
        });
        group.wait();
    });
    REQUIRE(actual.weights.size() == reference.weights.size());
    CHECK(actual.tails == reference.tails);
    for (std::size_t index = 0; index < actual.weights.size(); ++index) {
        CHECK(actual.weights[index].mass == reference.weights[index].mass);
        CHECK(actual.weights[index].log_mass == reference.weights[index].log_mass);
        CHECK(actual.weights[index].conditional == reference.weights[index].conditional);
    }
    CHECK(values == expected);
    CHECK(other_values == expected_other);
}
