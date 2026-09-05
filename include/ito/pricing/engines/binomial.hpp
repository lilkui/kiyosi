#pragma once

#include <ito/pricing/result.hpp>
#include <ito/market/context.hpp>
#include <ito/instruments/vanilla.hpp>

namespace ito {

struct BinomialAmericanSettings {
    int steps = 256;
};

/// Cox-Ross-Rubinstein American engine.
/// Value is tree-derived; delta and gamma are numerical tree estimates; higher Greeks are unsupported and zero.
class BinomialAmericanEngine {
public:
    static constexpr risk_measure_set supported_risk_measures =
        risk_bit(risk_measure::price) | risk_bit(risk_measure::delta) | risk_bit(risk_measure::gamma);

    explicit BinomialAmericanEngine(BinomialAmericanSettings settings = {}) : settings_(settings) {}
    explicit BinomialAmericanEngine(int steps) : settings_{steps} {}

    [[nodiscard]] result<PricingResult> price(const EuropeanOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const EuropeanOption&, const PricingContext&, BinomialAmericanSettings) const;
    [[nodiscard]] result<PricingResult> price(const AmericanOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const AmericanOption&, const PricingContext&, PricingRequest) const;
    [[nodiscard]] result<PricingResult> price(const AmericanOption&, const PricingContext&, BinomialAmericanSettings) const;
    [[nodiscard]] result<PricingResult> price(
        const AmericanOption&, const PricingContext&, BinomialAmericanSettings, PricingRequest) const;
    [[nodiscard]] result<PricingResult> price(
        const EuropeanOption&, const PricingContext&, PricingRequest) const;

    BinomialAmericanSettings settings() const noexcept { return settings_; }

private:
    BinomialAmericanSettings settings_;
};

} // namespace ito

