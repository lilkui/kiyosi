#pragma once

#include <kiyosi/instruments/structured/phoenix.hpp>
#include <kiyosi/instruments/structured/snowball.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

template <typename Note>
[[nodiscard]] KIYOSI_EXPORT result<PricingResult> price_autocallable_finite_difference(
    const Note&, const PricingContext&, FiniteDifferenceSettings);

/// One- or two-layer backward induction, depending on whether the note has knock-in state, with
/// knock-out and coupon events anchored onto the time grid.
template <typename Note>
class KIYOSI_EXPORT FiniteDifferenceStructuredEngine {
public:
    explicit FiniteDifferenceStructuredEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    FiniteDifferenceStructuredEngine(int asset_steps, int time_steps,
                                     finite_difference_scheme scheme = FiniteDifferenceSettings{}.scheme)
        : settings_{asset_steps, time_steps, scheme} {}

    [[nodiscard]] result<PricingResult> price(const Note& note, const PricingContext& context) const
    {
        return price_autocallable_finite_difference(note, context, settings_);
    }

    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    FiniteDifferenceSettings settings_;
};

using FiniteDifferencePhoenixEngine = FiniteDifferenceStructuredEngine<PhoenixOption>;
using FiniteDifferenceSnowballEngine = FiniteDifferenceStructuredEngine<SnowballOption>;
using FiniteDifferenceBinarySnowballEngine = FiniteDifferenceStructuredEngine<BinarySnowballOption>;
using FiniteDifferenceTernarySnowballEngine = FiniteDifferenceStructuredEngine<TernarySnowballOption>;

} // namespace kiyosi
