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
class RangeAccrualOption;
class BarrierOption;
class DoubleBarrierOption;
class BinaryBarrierOption;
class TouchOption;
class DoubleTouchOption;
class CoxRossRubinsteinVanillaEngine;
class CoxRossRubinsteinDigitalEngine;
class CoxRossRubinsteinBarrierEngine;
class MonteCarloAutocallableEngine;

namespace detail {

// Declare built-in hooks before generic solvers so either public-header include order works.
template <typename Option>
Result<ImpliedVolatilitySettings> prepare_implied_volatility(
    const CoxRossRubinsteinVanillaEngine&, const Option&, const PricingContext&, ImpliedVolatilitySettings);
template <typename Option>
Result<ImpliedVolatilitySettings> prepare_implied_volatility(
    const CoxRossRubinsteinDigitalEngine&, const Option&, const PricingContext&, ImpliedVolatilitySettings);
template <AutocallableProduct Note>
[[nodiscard]] KIYOSI_EXPORT Result<ImpliedVolatilitySettings> prepare_implied_volatility(
    const MonteCarloAutocallableEngine&, const Note&, const PricingContext&, ImpliedVolatilitySettings);

template <typename Option>
Result<ImpliedVolatilitySettings> prepare_implied_volatility(
    const CoxRossRubinsteinBarrierEngine&, const Option&, const PricingContext&, ImpliedVolatilitySettings);
template <AutocallableProduct Note>
Result<std::array<double, 2>> coupon_endpoint_prices(
    const MonteCarloAutocallableEngine&, const Note&, const Note&, const PricingContext&);

template <typename Option>
concept BarrierProduct = std::same_as<Option, BarrierOption> || std::same_as<Option, DoubleBarrierOption> ||
                         std::same_as<Option, BinaryBarrierOption> || std::same_as<Option, TouchOption> || std::same_as<Option, DoubleTouchOption>;

[[nodiscard]] KIYOSI_EXPORT bool at_spot_discontinuity(const Accumulator&, const PricingContext&, double radius = 0.0);
[[nodiscard]] KIYOSI_EXPORT Result<bool> time_stencil_available(const Accumulator&, const PricingContext&, Timestamp, Timestamp);
[[nodiscard]] KIYOSI_EXPORT bool volatility_affects_price(const Accumulator&, const PricingContext&);

[[nodiscard]] KIYOSI_EXPORT bool at_spot_discontinuity(const RangeAccrualOption&, const PricingContext&, double radius = 0.0);
[[nodiscard]] KIYOSI_EXPORT Result<bool> time_stencil_available(const RangeAccrualOption&, const PricingContext&, Timestamp, Timestamp);
[[nodiscard]] KIYOSI_EXPORT bool volatility_affects_price(const RangeAccrualOption&, const PricingContext&);

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

} // namespace detail
} // namespace kiyosi
