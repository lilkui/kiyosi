#include "quadrature.hpp"

#if KIYOSI_HAS_AVX2
#include <xsimd/config/xsimd_cpu_features.hpp>
#endif

namespace kiyosi::detail::quadrature {

Transition transition_weights(const std::vector<double>& nodes, double drift, double deviation)
{
    static const auto weights = [] {
#if KIYOSI_HAS_AVX2
        if (xsimd::cpu_features{}.avx2()) return &transition_weights_avx2;
#endif
        return &transition_weights_scalar;
    }();
    return weights(nodes, drift, deviation);
}

void integrate_curves(const Curve& curve, const Transition& transition, double log_discount,
                      std::span<double> values)
{
    static const auto integrate = [] {
#if KIYOSI_HAS_AVX2
        if (xsimd::cpu_features{}.avx2()) return &integrate_curves_avx2;
#endif
        return +[](const Curve& curve, const Transition& transition, double log_discount, std::span<double> values) {
            integrate_curves_scalar(curve, transition, log_discount, values);
        };
    }();
    integrate(curve, transition, log_discount, values);
}

} // namespace kiyosi::detail::quadrature
