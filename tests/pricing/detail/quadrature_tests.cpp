#include <catch2/catch_test_macros.hpp>
#include <catch2/generators/catch_generators.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <array>
#include <cmath>
#include <limits>
#include <numbers>
#include <utility>
#include <vector>

#include <oneapi/tbb/task_arena.h>
#include <oneapi/tbb/task_group.h>

#include "pricing/detail/quadrature.hpp"

namespace {
using kiyosi::detail::scaled_exponential;
using namespace kiyosi::detail::quadrature;

Transition dense_transition(std::vector<IntervalMoments> weights, std::size_t cells)
{
    const auto targets = weights.size() / cells;
    Transition result{std::move(weights), std::vector<std::array<double, 2>>(targets, {-infinity, -infinity}), {}};
    for (std::size_t target = 0; target < targets; ++target)
        result.rows.push_back({0, target * cells, cells});
    result.active_cell_count = result.weights.size();
    return result;
}

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

TEST_CASE("Quadrature cell assets follow geometric midpoints on irregular grids", "[quadrature-kernel]")
{
    const double log_two = std::numbers::ln2;
    const std::vector<double> nodes{-2.0 * log_two, -log_two, log_two, 3.0 * log_two};
    for (const double anchor : {100.0, 1e300}) {
        const auto assets = cell_assets(nodes, anchor);
        REQUIRE(assets.size() == 3);
        CHECK_THAT(assets[0], Catch::Matchers::WithinRel(anchor / std::sqrt(8.0), 1e-14));
        CHECK(assets[1] == anchor);
        CHECK_THAT(assets[2], Catch::Matchers::WithinRel(4.0 * anchor, 1e-14));
    }
}

TEST_CASE("Quadrature terminal integration preserves asset-only and empty pieces", "[quadrature-kernel]")
{
    const double deviation = 0.2;
    const double drift = 0.01;
    const double boundary = 0.1;
    const double asset_probability = 0.5 * std::erfc(-(boundary - drift - deviation * deviation) / (deviation * std::numbers::sqrt2));
    for (const auto [discount, asset] : {std::pair{-0.03, 2.0}, {-800.0, 1e300}, {710.0, 1e-300}}) {
        CAPTURE(discount, asset);
        const double actual = integrate_terminal({{-infinity, boundary, 0.0, asset}, {boundary, infinity, 0.0, 0.0}}, 0.0, 100.0, drift, deviation, discount);
        const double expected = std::exp(std::log(asset) + std::log(100.0) + drift + 0.5 * deviation * deviation + discount + std::log(asset_probability));
        CHECK_THAT(actual, Catch::Matchers::WithinRel(expected, 1e-12));
        CHECK(integrate_terminal({{-infinity, infinity, 0.0, 0.0}}, 0.0, 100.0, drift, deviation, discount) == 0.0);
    }
    const double cash_probability = 0.5 * std::erfc(-(boundary - drift) / (deviation * std::numbers::sqrt2));
    CHECK_THAT(integrate_terminal({{-infinity, boundary, 2.0, 0.0}}, 0.0, 100.0, drift, deviation, -0.03),
               Catch::Matchers::WithinRel(2.0 * std::exp(-0.03) * cash_probability, 1e-12));
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

TEST_CASE("Prepared quadrature interpolation reproduces cubics and reuses curve storage", "[quadrature-kernel]")
{
    const std::vector<double> nodes{-1.0, -0.75, 0.0, 0.5, 2.0};
    const CubicInterpolation interpolation{nodes};
    Curve curve;
    for (const double scale : {1.0, -2.0, 0.0}) {
        std::vector<double> values;
        values.reserve(nodes.size());
        for (const double node : nodes)
            values.push_back(scale * (node * node * node - 2.0 * node + 1.0));
        interpolation.interpolate(values, scale - 3.0, scale + 4.0, curve);
        REQUIRE(curve.cells.size() == nodes.size() - 1);
        CHECK(curve.low == scale - 3.0);
        CHECK(curve.high == scale + 4.0);
        for (std::size_t cell = 0; cell < curve.cells.size(); ++cell)
            for (const double weight : {0.0, 0.25, 0.75, 1.0}) {
                const double point = std::lerp(nodes[cell], nodes[cell + 1], weight);
                const auto& polynomial = curve.cells[cell];
                const double actual = polynomial[0] + weight * (polynomial[1] + weight * (polynomial[2] + weight * polynomial[3]));
                CHECK_THAT(actual, Catch::Matchers::WithinAbs(scale * (point * point * point - 2.0 * point + 1.0), 1e-13));
            }
        const auto* storage = curve.cells.data();
        interpolation.interpolate(values, 1.0, 2.0, curve);
        CHECK(curve.cells.data() == storage);
        CHECK(curve.low == 1.0);
        CHECK(curve.high == 2.0);
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
    REQUIRE(transition.rows.size() == nodes.size());
    CHECK(transition.weights.size() <= nodes.size() * curve.cells.size());
    std::vector<double> actual(nodes.size());
    integrate_curves(curve, transition, -0.001, actual);
    for (std::size_t target = 0; target < nodes.size(); ++target) {
        std::vector<IntervalMoments> reference;
        for (std::size_t cell = 0; cell + 1 < nodes.size(); ++cell) {
            reference.push_back(cell_moments(nodes[cell], nodes[cell + 1], nodes[target] + 0.0001, deviation));
            const auto row = transition.rows[target];
            if (cell >= row.first && cell < row.first + row.count) {
                const auto& actual_moments = transition.moment(row, cell - row.first);
                CHECK(actual_moments.mass == reference.back().mass);
                CHECK(actual_moments.log_mass == reference.back().log_mass);
                CHECK(actual_moments.conditional == reference.back().conditional);
            } else {
                CHECK(reference.back().log_mass < -1500.0);
            }
        }
        const double mean = nodes[target] + 0.0001;
        const std::array tails{log_normal_cdf((nodes.front() - mean) / deviation), log_normal_cdf((mean - nodes.back()) / deviation)};
        const double expected = reference_integral(curve, reference, tails, -0.001);
        CAPTURE(count, deviation, target, expected);
        CHECK(actual[target] == expected);
    }
}

TEST_CASE("Quadrature reused moments preserve exact relative cell geometry", "[quadrature-kernel]")
{
    for (const double origin : {-1.0, 1024.3})
        for (const double drift : {0.0, 0.0001, -3.0, 3.0, -8.0, 8.0})
            for (const bool irregular : {false, true}) {
                auto nodes = integration_nodes(origin, origin + 2.0, 64, {}).value();
                if (irregular) {
                    nodes[31] = std::nextafter(nodes[31], nodes[32]);
                    nodes.insert(nodes.begin() + 12, std::midpoint(nodes[11], nodes[12]));
                }
                const auto transition = transition_weights(nodes, drift, 0.02);
                if (std::abs(drift) == 8.0) CHECK(transition.weights.empty());
                for (std::size_t target = 0; target < nodes.size(); ++target) {
                    const double mean = nodes[target] + drift;
                    const std::array tails{kiyosi::detail::log_normal_cdf((nodes.front() - mean) / 0.02),
                                           kiyosi::detail::log_normal_cdf((mean - nodes.back()) / 0.02)};
                    CHECK(transition.tails[target] == tails);
                    const auto row = transition.rows[target];
                    for (std::size_t index = 0; index < row.count; ++index) {
                        const auto cell = row.first + index;
                        const auto expected = cell_moments(nodes[cell], nodes[cell + 1], nodes[target] + drift, 0.02);
                        const auto& actual = transition.moment(row, index);
                        CHECK(actual.mass == expected.mass);
                        CHECK(actual.log_mass == expected.log_mass);
                        CHECK(actual.conditional == expected.conditional);
                    }
                }
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
    const auto transition = transition_weights(nodes, GENERATE(-3.0, 0.0001, 3.0), GENERATE(0.00001, 0.02));
    for (const double discount : {-1500.0, -750.0, -0.001, 700.0}) {
        std::vector<double> expected_first(nodes.size()), expected_second(nodes.size());
        integrate_curves(first, transition, discount, expected_first);
        integrate_curves(second, transition, discount, expected_second);
        std::vector<double> actual_first(nodes.size()), actual_second(nodes.size());
        integrate_curve_pair(first, second, transition, discount, actual_first, actual_second);
        CHECK(actual_first == expected_first);
        CHECK(actual_second == expected_second);
    }
}

TEST_CASE("Quadrature scheduling counts active cells before weight sharing", "[quadrature-kernel][quadrature-work]")
{
    const auto nodes = integration_nodes(-1.0, 1.0, 128, {}).value();
    const auto narrow = transition_weights(nodes, 0.0, 0.00001);
    CHECK(narrow.active_cell_count == 256);
    CHECK(narrow.weights.size() < narrow.active_cell_count);
    const auto empty = transition_weights(nodes, 3.0, 0.00001);
    CHECK(empty.active_cell_count == 0);
    CHECK(empty.weights.empty());
}

TEST_CASE("Quadrature compact rows preserve irregular-grid tails and tiny recoverable masses", "[quadrature-kernel]")
{
    const auto nodes = integration_nodes(-1.0, 1.0, 512, std::array{-0.913, 0.017, 0.321}).value();
    const double deviation = 0.2 / std::sqrt(365.0);
    const double drift = GENERATE(-3.0, 0.0001, 3.0);
    const auto transition = transition_weights(nodes, drift, deviation);
    CHECK(transition.weights.size() < nodes.size() * (nodes.size() - 1) * 0.6);
    const Curve curve{std::vector<Polynomial>(nodes.size() - 1, {1e300, -1e299, 1e298, -1e297}), -1e299, 1e299};
    std::vector<double> actual(nodes.size());
    integrate_curves(curve, transition, -740.0, actual);
    for (std::size_t target = 0; target < nodes.size(); ++target) {
        std::vector<IntervalMoments> dense;
        for (std::size_t cell = 0; cell + 1 < nodes.size(); ++cell)
            dense.push_back(cell_moments(nodes[cell], nodes[cell + 1], nodes[target] + drift, deviation));
        CHECK(actual[target] == reference_integral(curve, dense, transition.tails[target], -740.0));
    }
}

TEST_CASE("Quadrature translated cells share weight storage without changing integration", "[quadrature-kernel]")
{
    const auto nodes = integration_nodes(-1.0, 1.0, 512, {}).value();
    const double deviation = 0.2 / std::sqrt(365.0);
    const auto transition = transition_weights(nodes, 0.0, deviation);
    REQUIRE(transition.weights.size() < 2 * nodes.size());
    CHECK(transition.weights.size() * sizeof(IntervalMoments) < nodes.size() * (nodes.size() - 1) * sizeof(IntervalMoments) / 100);
    const Curve curve{std::vector<Polynomial>(nodes.size() - 1, {1e300, -1e299, 1e298, -1e297}), -1e299, 1e299};
    std::vector<double> actual(nodes.size());
    integrate_curves(curve, transition, -740.0, actual);
    for (const std::size_t target : {std::size_t{0}, nodes.size() / 2, nodes.size() - 1}) {
        std::vector<IntervalMoments> dense;
        for (std::size_t cell = 0; cell + 1 < nodes.size(); ++cell)
            dense.push_back(cell_moments(nodes[cell], nodes[cell + 1], nodes[target], deviation));
        CHECK(actual[target] == reference_integral(curve, dense, transition.tails[target], -740.0));
    }
}

TEST_CASE("Quadrature rounding variants share exact moments without dense weight storage", "[quadrature-kernel]")
{
    const auto nodes = integration_nodes(-1.0, 1.0, 513, {}).value();
    const double deviation = 0.2 / std::sqrt(365.0);
    const double drift = GENERATE(0.0, 0.0001);
    const auto transition = transition_weights(nodes, drift, deviation);
    REQUIRE_FALSE(transition.indices.empty());
    const auto bytes = transition.weights.size() * sizeof(IntervalMoments) + transition.indices.size() * sizeof(std::uint32_t);
    CHECK(bytes < transition.active_cell_count * sizeof(IntervalMoments) / 3);
    for (std::size_t target = 0; target < nodes.size(); ++target) {
        const auto& row = transition.rows[target];
        for (std::size_t index = 0; index < row.count; ++index) {
            const auto cell = row.first + index;
            const auto expected = cell_moments(nodes[cell], nodes[cell + 1], nodes[target] + drift, deviation);
            const auto& actual = transition.moment(row, index);
            CHECK(actual.mass == expected.mass);
            CHECK(actual.log_mass == expected.log_mass);
            CHECK(actual.conditional == expected.conditional);
        }
    }
}

TEST_CASE("Quadrature off-grid spot windows exactly preserve full-grid integration", "[quadrature-kernel][quadrature-spot-window]")
{
    const auto nodes = integration_nodes(-1.0, 1.0, 512, std::array{-0.913, 0.017, 0.321}).value();
    const double mean = GENERATE(-3.0, -0.91, 0.0171, 0.32, 3.0);
    const double deviation = GENERATE(1e-6, 0.01, 0.2);
    const auto [first, last] = active_cells(nodes, mean, deviation);
    REQUIRE(first <= last);
    REQUIRE(last < nodes.size());
    if (deviation == 1e-6) CHECK(last - first <= 3);
    std::vector<IntervalMoments> dense;
    for (std::size_t cell = 0; cell + 1 < nodes.size(); ++cell) {
        dense.push_back(cell_moments(nodes[cell], nodes[cell + 1], mean, deviation));
        if (cell < first || cell >= last) CHECK(dense.back().log_mass < -1500.0);
    }
    const auto weights = std::span{dense}.subspan(first, last - first);
    const std::array tails{kiyosi::detail::log_normal_cdf((nodes.front() - mean) / deviation),
                           kiyosi::detail::log_normal_cdf((mean - nodes.back()) / deviation)};
    for (const auto [scale, discount] : {std::pair{1.0, 0.0}, {1e300, -740.0}, {1e-300, 710.0}}) {
        Curve curve{std::vector<Polynomial>(nodes.size() - 1), -scale, scale};
        for (std::size_t cell = 0; cell < curve.cells.size(); ++cell)
            curve.cells[cell] = {scale * (1.0 + static_cast<double>(cell) / static_cast<double>(curve.cells.size())), -0.1 * scale, 0.01 * scale, -0.001 * scale};
        const double expected = reference_integral(curve, dense, tails, discount);
        REQUIRE(std::isfinite(expected));
        CHECK(integrate_curve(curve, weights, tails, discount, first) == expected);
    }
}

TEST_CASE("Quadrature integration retains compensated cancellation in independent targets", "[quadrature-kernel]")
{
    const Curve curve{{Polynomial{1e16, 0.0, 0.0, 0.0}, Polynomial{1.0, 0.0, 0.0, 0.0}, Polynomial{-1e16, 0.0, 0.0, 0.0}}, 0.0, 0.0};
    auto transition = dense_transition(std::vector<IntervalMoments>(7 * curve.cells.size()), curve.cells.size());
    for (std::size_t target = 0; target < transition.tails.size(); ++target)
        for (std::size_t cell = 0; cell < curve.cells.size(); ++cell) {
            const double mass = 0.5 * static_cast<double>(target + 1);
            transition.weights[target * curve.cells.size() + cell] = {mass, std::log(mass), {}};
        }
    std::array<double, 7> values{};
    integrate_curves(curve, transition, 0.0, values);
    for (std::size_t target = 0; target < values.size(); ++target)
        CHECK(values[target] == 0.5 * static_cast<double>(target + 1));
    auto second = curve;
    second.cells[1][0] = 3.0;
    std::array<double, 7> second_values{};
    integrate_curve_pair(curve, second, transition, 0.0, values, second_values);
    for (std::size_t target = 0; target < values.size(); ++target) {
        CHECK(values[target] == 0.5 * static_cast<double>(target + 1));
        CHECK(second_values[target] == 1.5 * static_cast<double>(target + 1));
    }
}

TEST_CASE("Quadrature integration recovers subnormal masses and isolates discarded targets", "[quadrature-kernel]")
{
    const Curve curve{{Polynomial{1e300, 0.0, 0.0, 0.0}}, 0.0, 0.0};
    const double subnormal = std::numeric_limits<double>::denorm_min();
    const std::array weights{
        IntervalMoments{0.5, std::log(0.5), {}}, IntervalMoments{subnormal, std::log(subnormal), {}},
        IntervalMoments{0.0, -900.0, {}}, IntervalMoments{0.0, -1600.0, {std::numeric_limits<double>::quiet_NaN(), 0.0, 0.0}},
        IntervalMoments{0.0, -800.0, {}}, IntervalMoments{0.0, -1500.0, {}}, IntervalMoments{0.0, -infinity, {}}};
    const auto transition = dense_transition({weights.begin(), weights.end()}, 1);
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
    const auto transition = dense_transition({weights.begin(), weights.end()}, 1);
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
    const double deviation = GENERATE(0.00001, 0.02);
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
        reference = transition_weights(nodes, 0.0001, deviation);
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
            actual = transition_weights(nodes, 0.0001, deviation);
            integrate_curve_pair(curve, other, actual, -0.001, values, other_values);
        });
        group.wait();
    });
    REQUIRE(actual.weights.size() == reference.weights.size());
    CHECK(actual.tails == reference.tails);
    CHECK(actual.rows == reference.rows);
    for (std::size_t index = 0; index < actual.weights.size(); ++index) {
        CHECK(actual.weights[index].mass == reference.weights[index].mass);
        CHECK(actual.weights[index].log_mass == reference.weights[index].log_mass);
        CHECK(actual.weights[index].conditional == reference.weights[index].conditional);
    }
    CHECK(values == expected);
    CHECK(other_values == expected_other);
}
