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
    explicit FiniteDifferenceEuropeanEngine(FiniteDifferenceSettings settings = {})
        : settings_(settings) {}
    FiniteDifferenceEuropeanEngine(int asset_steps, int time_steps,
                                   finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson)
        : settings_{asset_steps, time_steps, scheme} {}

    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        return price_impl(option, context);
    }
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] result<PricingResult> price_impl(
        const EuropeanOption&, const PricingContext&) const;
    FiniteDifferenceSettings settings_;
};

/// Uniform-grid finite-difference American engine with early exercise at every time layer.
class FiniteDifferenceAmericanEngine {
public:
    explicit FiniteDifferenceAmericanEngine(FiniteDifferenceSettings settings = {})
        : settings_(settings) {}
    FiniteDifferenceAmericanEngine(int asset_steps, int time_steps,
                                   finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson)
        : settings_{asset_steps, time_steps, scheme} {}

    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, AmericanExercise>
    [[nodiscard]] result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        return price_impl(option, context);
    }
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] result<PricingResult> price_impl(
        const AmericanOption&, const PricingContext&) const;
    FiniteDifferenceSettings settings_;
};
using FdEuropeanEngine = FiniteDifferenceEuropeanEngine;
using FdAmericanEngine = FiniteDifferenceAmericanEngine;

} // namespace kiyosi
