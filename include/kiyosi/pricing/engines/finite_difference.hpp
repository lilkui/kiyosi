#pragma once

#include <kiyosi/pricing/result.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/instruments/vanilla.hpp>

namespace kiyosi {

enum class finite_difference_scheme : unsigned char {
    explicit_euler,
    implicit_euler,
    crank_nicolson,
};

struct FiniteDifferenceSettings {
    int asset_steps = 200;
    int time_steps = 200;
    finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson;
    double upper_boundary = 0.0;
};

/// Uniform-grid finite-difference European engine for vanilla options.
class FiniteDifferenceEuropeanEngine {
public:
    static constexpr risk_measure_set supported_risk_measures =
        risk_bit(risk_measure::price) | risk_bit(risk_measure::delta) | risk_bit(risk_measure::gamma);

    explicit FiniteDifferenceEuropeanEngine(FiniteDifferenceSettings settings = {})
        : settings_(settings) {}
    FiniteDifferenceEuropeanEngine(int asset_steps, int time_steps,
                                   finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson)
        : settings_{asset_steps, time_steps, scheme} {}

    [[nodiscard]] result<PricingResult> price(const EuropeanOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const EuropeanOption&, const PricingContext&, FiniteDifferenceSettings) const;
    [[nodiscard]] result<PricingResult> price(
        const EuropeanOption&, const PricingContext&, PricingRequest) const;
    [[nodiscard]] result<PricingResult> price(
        const EuropeanOption&, const PricingContext&, FiniteDifferenceSettings, PricingRequest) const;
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    FiniteDifferenceSettings settings_;
};

/// Uniform-grid finite-difference American engine with early exercise at every time layer.
class FiniteDifferenceAmericanEngine {
public:
    static constexpr risk_measure_set supported_risk_measures =
        risk_bit(risk_measure::price) | risk_bit(risk_measure::delta) | risk_bit(risk_measure::gamma);

    explicit FiniteDifferenceAmericanEngine(FiniteDifferenceSettings settings = {})
        : settings_(settings) {}
    FiniteDifferenceAmericanEngine(int asset_steps, int time_steps,
                                   finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson)
        : settings_{asset_steps, time_steps, scheme} {}

    [[nodiscard]] result<PricingResult> price(const EuropeanOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const EuropeanOption&, const PricingContext&, FiniteDifferenceSettings) const;
    [[nodiscard]] result<PricingResult> price(const AmericanOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const AmericanOption&, const PricingContext&, PricingRequest) const;
    [[nodiscard]] result<PricingResult> price(const AmericanOption&, const PricingContext&, FiniteDifferenceSettings) const;
    [[nodiscard]] result<PricingResult> price(
        const AmericanOption&, const PricingContext&, FiniteDifferenceSettings, PricingRequest) const;
    [[nodiscard]] result<PricingResult> price(
        const EuropeanOption&, const PricingContext&, PricingRequest) const;
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi

