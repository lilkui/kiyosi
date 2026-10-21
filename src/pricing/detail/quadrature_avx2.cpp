#include "quadrature.hpp"

#include <xsimd/xsimd.hpp>

namespace kiyosi::detail::quadrature {

Transition transition_weights_avx2(const std::vector<double>& nodes, double drift, double deviation)
{
    using Batch = xsimd::batch<double, xsimd::avx2>;
    static_assert(Batch::size == transition_lanes);
    const std::size_t cells = nodes.size() - 1;
    Transition transition;
    transition.weights.resize(((nodes.size() + transition_lanes - 1) / transition_lanes) * cells);
    transition.tails.reserve(nodes.size());
    for (const double node : nodes) {
        const double mean = node + drift;
        transition.tails.push_back({log_normal_cdf((nodes.front() - mean) / deviation),
                                    log_normal_cdf((mean - nodes.back()) / deviation)});
    }
    for (std::size_t target = 0; target < nodes.size(); target += Batch::size) {
        for (std::size_t cell = 0; cell < cells; ++cell) {
            auto& block = transition.weights[(target / transition_lanes) * cells + cell];
            const double left = nodes[cell];
            const double right = nodes[cell + 1];
            const double half = (0.5 * (right - left)) / deviation;
            if (target + Batch::size <= nodes.size() && half > 0.0) {
                const Batch mean = Batch::load_unaligned(nodes.data() + target) + Batch{drift};
                const Batch center = (Batch{std::midpoint(left, right)} - mean) / Batch{deviation};
                if (xsimd::all(xsimd::abs(center) * Batch{half} + Batch{0.5 * half * half} < Batch{0.25})) {
                    std::array<Batch, 4> integrals{Batch{0.0}, Batch{0.0}, Batch{0.0}, Batch{0.0}};
                    Batch previous{0.0}, term{1.0};
                    for (int order = 0; order < 24; ++order) {
                        for (int power = 0; power < 4; ++power)
                            if ((order + power) % 2 == 0)
                                integrals[static_cast<std::size_t>(power)] += Batch{2.0} * term / Batch{order + power + 1.0};
                        const Batch next = (-center * Batch{half} * term - Batch{half * half} * previous) / Batch{order + 1.0};
                        previous = term;
                        term = next;
                    }
                    const Batch first = integrals[1] / integrals[0];
                    const Batch second = integrals[2] / integrals[0];
                    const Batch third = integrals[3] / integrals[0];
                    (Batch{0.5} * (Batch{1.0} + first)).store_unaligned(block.conditional[0].data());
                    (Batch{0.25} * (Batch{1.0} + Batch{2.0} * first + second)).store_unaligned(block.conditional[1].data());
                    (Batch{0.125} * (Batch{1.0} + Batch{3.0} * first + Batch{3.0} * second + third)).store_unaligned(block.conditional[2].data());
                    std::array<double, Batch::size> centers{}, normalizers{};
                    center.store_unaligned(centers.data());
                    integrals[0].store_unaligned(normalizers.data());
                    const double log_half = std::log(half);
                    for (std::size_t lane = 0; lane < Batch::size; ++lane) {
                        // Scalar transcendental functions retain the original tail and subnormal behavior.
                        block.log_mass[lane] = -0.5 * centers[lane] * centers[lane] + std::log(inverse_sqrt_two_pi) +
                                               log_half + std::log(normalizers[lane]);
                        block.mass[lane] = std::exp(block.log_mass[lane]);
                    }
                    continue;
                }
            }
            for (std::size_t lane = 0; lane < Batch::size && target + lane < nodes.size(); ++lane)
                block.set_lane(lane, cell_moments(left, right, nodes[target + lane] + drift, deviation));
        }
    }
    return transition;
}

void integrate_curves_avx2(const Curve& curve, const Transition& transition, double log_discount,
                           std::span<double> values)
{
    const double discount = std::exp(log_discount);
    if (!std::isnormal(discount)) {
        integrate_curves_scalar(curve, transition, log_discount, values);
        return;
    }
    using Batch = xsimd::batch<double, xsimd::avx2>;
    static_assert(Batch::size == transition_lanes);
    const std::size_t cells = curve.cells.size();
    std::size_t target = 0;
    for (; target + Batch::size <= values.size(); target += Batch::size) {
        std::array<double, Batch::size> initial{}, corrections{};
        for (std::size_t lane = 0; lane < Batch::size; ++lane) {
            Sum sum;
            sum.add(signed_exponential(curve.low, transition.tails[target + lane][0] + log_discount));
            sum.add(signed_exponential(curve.high, transition.tails[target + lane][1] + log_discount));
            initial[lane] = sum.value;
            corrections[lane] = sum.correction;
        }
        Batch value = Batch::load_unaligned(initial.data());
        Batch correction = Batch::load_unaligned(corrections.data());
        for (std::size_t cell = 0; cell < cells; ++cell) {
            const auto& block = transition.weights[(target / transition_lanes) * cells + cell];
            const Batch log_mass = Batch::load_unaligned(block.log_mass.data());
            if (xsimd::all(log_mass < Batch{-1500.0})) continue;
            const Batch mass = Batch::load_unaligned(block.mass.data());
            const auto& p = curve.cells[cell];
            const Batch mean = Batch{p[0]} + Batch{p[1]} * Batch::load_unaligned(block.conditional[0].data()) +
                               Batch{p[2]} * Batch::load_unaligned(block.conditional[1].data()) +
                               Batch{p[3]} * Batch::load_unaligned(block.conditional[2].data());
            Batch term = xsimd::select(log_mass < Batch{-1500.0}, Batch{0.0}, mean * mass * Batch{discount});
            const auto underflows = (xsimd::abs(mean) < Batch{underflow_mean_limit}) & (log_mass < Batch{underflow_log_mass});
            if (xsimd::any((log_mass >= Batch{-1500.0}) & (mass == Batch{0.0}) & !underflows)) {
                std::array<double, Batch::size> recovered{};
                term.store_unaligned(recovered.data());
                for (std::size_t lane = 0; lane < Batch::size; ++lane) {
                    const auto weight = block.lane(lane);
                    const bool underflows_lane = std::abs(mean.get(lane)) < underflow_mean_limit && weight.log_mass < underflow_log_mass;
                    if (weight.mass == 0.0 && weight.log_mass >= -1500.0 &&
                        !underflows_lane)
                        recovered[lane] = weight.integrate(p) * discount;
                }
                term = Batch::load_unaligned(recovered.data());
            }
            // Each lane keeps its target's original source order and compensated sum.
            const Batch next = value + term;
            correction += xsimd::select(xsimd::abs(value) >= xsimd::abs(term), (value - next) + term, (term - next) + value);
            value = next;
        }
        (value + correction).store_unaligned(values.data() + target);
    }
    if (target < values.size()) {
        integrate_curves_scalar(curve, transition, log_discount, values, target);
    }
}

} // namespace kiyosi::detail::quadrature
