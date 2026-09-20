#pragma once

#include <kiyosi/instruments/structured/phoenix.hpp>
#include <kiyosi/instruments/structured/snowball.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

template <typename Note>
[[nodiscard]] KIYOSI_EXPORT Result<PricingResult> price_autocallable_finite_difference(
    const Note&, const PricingContext&, FiniteDifferenceSettings);

/// One- or two-layer backward induction, depending on whether the note has knock-in state, with
/// knock-out and coupon events anchored onto the time grid.
template <typename Note>
class KIYOSI_EXPORT FiniteDifferenceAutocallableEngine {
public:
    explicit FiniteDifferenceAutocallableEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    FiniteDifferenceAutocallableEngine(int asset_step_count, int time_step_count,
                                       FiniteDifferenceScheme scheme = FiniteDifferenceSettings{}.scheme)
        : settings_{asset_step_count, time_step_count, scheme} {}

    [[nodiscard]] Result<PricingResult> price(const Note& note, const PricingContext& context) const
    {
        return price_autocallable_finite_difference(note, context, settings_);
    }

    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    FiniteDifferenceSettings settings_;
};

using FiniteDifferencePhoenixEngine = FiniteDifferenceAutocallableEngine<PhoenixOption>;
using FiniteDifferenceSnowballEngine = FiniteDifferenceAutocallableEngine<SnowballOption>;
using FiniteDifferenceBinarySnowballEngine = FiniteDifferenceAutocallableEngine<BinarySnowballOption>;
using FiniteDifferenceTernarySnowballEngine = FiniteDifferenceAutocallableEngine<TernarySnowballOption>;

} // namespace kiyosi
