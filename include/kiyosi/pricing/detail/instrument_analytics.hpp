#pragma once

#include <array>
#include <concepts>

#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>
#include <kiyosi/instruments/autocallable/fwd.hpp>
#include <kiyosi/instruments/asian.hpp>
#include <kiyosi/pricing/settings/implied.hpp>

namespace kiyosi {

class PricingContext;
class Accumulator;
class BarrierOption;
class BinaryBarrierOption;
class TouchOption;

namespace detail {

template <typename Option>
concept BarrierProduct = std::same_as<Option, BarrierOption> ||
                         std::same_as<Option, BinaryBarrierOption> || std::same_as<Option, TouchOption>;

[[nodiscard]] KIYOSI_EXPORT bool at_spot_discontinuity(const Accumulator&, const PricingContext&, double radius = 0.0);
[[nodiscard]] KIYOSI_EXPORT Result<bool> time_stencil_available(const Accumulator&, const PricingContext&, Timestamp, Timestamp);
[[nodiscard]] KIYOSI_EXPORT bool volatility_affects_price(const Accumulator&, const PricingContext&);

template <AveragingConvention Averaging>
[[nodiscard]] KIYOSI_EXPORT Result<bool> time_stencil_available(const AveragePriceOption<Averaging>&, const PricingContext&, Timestamp, Timestamp);
[[nodiscard]] KIYOSI_EXPORT bool volatility_affects_price(const AveragePriceOption<ArithmeticAveraging>&, const PricingContext&);

template <BarrierProduct Option>
[[nodiscard]] KIYOSI_EXPORT bool at_spot_discontinuity(const Option&, const PricingContext&, double radius = 0.0);
template <BarrierProduct Option>
[[nodiscard]] KIYOSI_EXPORT bool all_greeks_unavailable(const Option&, const PricingContext&);
template <BarrierProduct Option>
[[nodiscard]] KIYOSI_EXPORT Result<bool> time_stencil_available(const Option&, const PricingContext&, Timestamp, Timestamp);
template <BarrierProduct Option>
[[nodiscard]] KIYOSI_EXPORT bool volatility_affects_price(const Option&, const PricingContext&);

template <AutocallableProduct Note>
[[nodiscard]] KIYOSI_EXPORT bool at_spot_discontinuity(const Note&, const PricingContext&, double radius = 0.0);
template <AutocallableProduct Note>
[[nodiscard]] KIYOSI_EXPORT Result<bool> time_stencil_available(const Note&, const PricingContext&, Timestamp, Timestamp);
template <AutocallableProduct Note>
[[nodiscard]] KIYOSI_EXPORT bool volatility_affects_price(const Note&, const PricingContext&);

template <AutocallableProduct Note>
    requires requires(const Note& note) { note.knock_out_coupon_rates(); }
[[nodiscard]] KIYOSI_EXPORT Result<Note> replace_coupon(const Note&, double, CouponQuoteConvention);
[[nodiscard]] KIYOSI_EXPORT Result<PhoenixOption> replace_coupon(const PhoenixOption&, double);

} // namespace detail
} // namespace kiyosi
