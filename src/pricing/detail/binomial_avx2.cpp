#include "binomial.hpp"

#include <xsimd/xsimd.hpp>

namespace kiyosi::detail {
namespace {

template <bool american>
bool rollback(std::span<double> values, double discount, double probability,
              double node_spot, double up_squared, double strike, double sign)
{
    using Batch = xsimd::batch<double, xsimd::avx2>;
    static_assert(Batch::size == 4);
    std::size_t index = 0;
    for (; index + Batch::size < values.size(); index += Batch::size) {
        const Batch left = Batch::load_unaligned(values.data() + index);
        const Batch right = Batch::load_unaligned(values.data() + index + 1);
        const Batch continuation = Batch{discount} * (Batch{probability} * right + Batch{1.0 - probability} * left);
        const Batch result = [&] {
            if constexpr (!american) return continuation;
            else {
                // Keep the scalar multiplication chain: Greeks amplify changes in asset-price rounding.
                const double second = node_spot * up_squared;
                const double third = second * up_squared;
                const double fourth = third * up_squared;
                const Batch spots{node_spot, second, third, fourth};
                node_spot = fourth * up_squared;
                const Batch exercise = Batch{sign} * (spots - Batch{strike});
                // Match std::max's first-operand handling of NaN and signed zero.
                return xsimd::select(continuation < exercise, exercise, continuation);
            }
        }();
        result.store_unaligned(values.data() + index);
        if (!xsimd::all(xsimd::isfinite(result))) return false;
    }
    return binomial_rollback_scalar<american>(values.subspan(index), discount, probability,
                                              node_spot, up_squared, strike, sign);
}

} // namespace

BinomialRollback binomial_rollback_avx2(bool american)
{
    return american ? &rollback<true> : &rollback<false>;
}

} // namespace kiyosi::detail
