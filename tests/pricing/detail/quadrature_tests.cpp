#include <catch2/catch_test_macros.hpp>
#include <catch2/generators/catch_generators.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <array>
#include <cmath>
#include <limits>
#include <numbers>
#include <vector>

#include "pricing/detail/quadrature.hpp"

namespace {
using namespace kiyosi::detail::quadrature;

double reference_integral(const Curve& curve, const std::vector<IntervalMoments>& weights,
                          const std::array<double, 2>& tails, double log_discount)
{
    Sum sum;
    sum.add(signed_exponential(curve.low, tails[0] + log_discount));
    sum.add(signed_exponential(curve.high, tails[1] + log_discount));
    const double discount = std::exp(log_discount);
    for (std::size_t cell = 0; cell < weights.size(); ++cell) {
        const auto& w = weights[cell];
        const auto& p = curve.cells[cell];
        const double mean = p[0] + p[1] * w.conditional[0] + p[2] * w.conditional[1] + p[3] * w.conditional[2];
        const double term = w.log_mass < -1500.0 ? 0.0 : w.mass > 0.0 ? mean * w.mass
                                                                      : signed_exponential(mean, w.log_mass);
        sum.add(std::isnormal(discount) ? term * discount : signed_exponential(term, log_discount));
    }
    return sum.total();
}

void set_weight(Transition& transition, std::size_t cells, std::size_t target, std::size_t cell, const IntervalMoments& weight)
{
    auto& block = transition.weights[(target / transition_lanes) * cells + cell];
    const auto lane = target % transition_lanes;
    block.mass[lane] = weight.mass;
    block.log_mass[lane] = weight.log_mass;
    for (std::size_t power = 0; power < weight.conditional.size(); ++power)
        block.conditional[power][lane] = weight.conditional[power];
}
} // namespace

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

TEST_CASE("Quadrature batches preserve scalar moments, tails and every remainder size", "[quadrature-kernel]")
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
    std::vector<double> actual(nodes.size()), scalar(nodes.size());
    integrate_curves(curve, transition, -0.001, actual);
    integrate_curves_scalar(curve, transition, -0.001, scalar);
    for (std::size_t target = 0; target < nodes.size(); ++target) {
        std::vector<IntervalMoments> reference;
        for (std::size_t cell = 0; cell + 1 < nodes.size(); ++cell) {
            reference.push_back(cell_moments(nodes[cell], nodes[cell + 1], nodes[target] + 0.0001, deviation));
            if (cell == std::min(target, nodes.size() - 2)) {
                const auto actual_moments = transition.weights[(target / transition_lanes) * curve.cells.size() + cell].lane(target % transition_lanes);
                CHECK(actual_moments.mass == reference.back().mass);
                CHECK(actual_moments.log_mass == reference.back().log_mass);
                CHECK(actual_moments.conditional == reference.back().conditional);
            }
        }
        const double mean = nodes[target] + 0.0001;
        const std::array tails{log_normal_cdf((nodes.front() - mean) / deviation), log_normal_cdf((mean - nodes.back()) / deviation)};
        const double expected = reference_integral(curve, reference, tails, -0.001);
        CAPTURE(count, deviation, target, expected);
        CHECK(actual[target] == expected);
        CHECK(scalar[target] == expected);
    }
}

TEST_CASE("Quadrature batches retain compensated cancellation in independent targets", "[quadrature-kernel]")
{
    const Curve curve{{Polynomial{1e16, 0.0, 0.0, 0.0}, Polynomial{1.0, 0.0, 0.0, 0.0}, Polynomial{-1e16, 0.0, 0.0, 0.0}}, 0.0, 0.0};
    Transition transition{std::vector<MomentBlock>(6), std::vector<std::array<double, 2>>(7, {-infinity, -infinity})};
    for (std::size_t target = 0; target < transition.tails.size(); ++target)
        for (std::size_t cell = 0; cell < curve.cells.size(); ++cell) {
            const double mass = 0.5 * static_cast<double>(target + 1);
            set_weight(transition, curve.cells.size(), target, cell, {mass, std::log(mass), {}});
        }
    std::array<double, 7> values{};
    integrate_curves(curve, transition, 0.0, values);
    for (std::size_t target = 0; target < values.size(); ++target)
        CHECK(values[target] == 0.5 * static_cast<double>(target + 1));
}

TEST_CASE("Quadrature batches recover subnormal masses and isolate discarded lanes", "[quadrature-kernel]")
{
    const Curve curve{{Polynomial{1e300, 0.0, 0.0, 0.0}}, 0.0, 0.0};
    const double subnormal = std::numeric_limits<double>::denorm_min();
    const std::array weights{
        IntervalMoments{0.5, std::log(0.5), {}}, IntervalMoments{subnormal, std::log(subnormal), {}},
        IntervalMoments{0.0, -900.0, {}}, IntervalMoments{0.0, -1600.0, {std::numeric_limits<double>::quiet_NaN(), 0.0, 0.0}},
        IntervalMoments{0.0, -800.0, {}}, IntervalMoments{0.0, -1500.0, {}}, IntervalMoments{0.0, -infinity, {}}};
    Transition transition{std::vector<MomentBlock>(2), std::vector<std::array<double, 2>>(weights.size(), {-infinity, -infinity})};
    for (std::size_t target = 0; target < weights.size(); ++target)
        set_weight(transition, 1, target, 0, weights[target]);
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

TEST_CASE("Quadrature batches preserve log-discount recovery and zero-recovery bounds", "[quadrature-kernel]")
{
    const double discount = GENERATE(0.0, -740.0, -800.0, -1500.0);
    const double coefficient = GENERATE(1.0, 1e19, 1e20, 1e300, -1e300);
    const Curve curve{{Polynomial{coefficient, 0.0, 0.0, 0.0}}, 0.0, 0.0};
    const std::array weights{IntervalMoments{1.0, 0.0, {}}, IntervalMoments{0.0, -801.0, {}},
                             IntervalMoments{0.0, -760.0, {}}, IntervalMoments{0.0, -799.0, {}}, IntervalMoments{0.0, -900.0, {}}};
    Transition transition{std::vector<MomentBlock>(2), std::vector<std::array<double, 2>>(weights.size(), {-infinity, -infinity})};
    for (std::size_t target = 0; target < weights.size(); ++target)
        set_weight(transition, 1, target, 0, weights[target]);
    std::array<double, weights.size()> values{};
    integrate_curves(curve, transition, discount, values);
    for (std::size_t target = 0; target < weights.size(); ++target) {
        CAPTURE(discount, coefficient, target);
        CHECK(values[target] == reference_integral(curve, {weights[target]}, transition.tails[target], discount));
    }
    if (discount == -740.0 && std::abs(coefficient) == 1e300) CHECK(std::abs(values[0]) > 1e-23);
    if (discount == 0.0 && coefficient == 1e19) CHECK(values[2] > 0.0);
}
