#pragma once

#include <concepts>
#include <type_traits>

namespace kiyosi {

/// Tag for exercise only at expiry.
struct EuropeanExercise {
    /// All European-exercise tags compare equal.
    friend bool operator==(const EuropeanExercise&, const EuropeanExercise&) = default;
};

/// Tag for exercise at any time during the option life.
struct AmericanExercise {
    /// All American-exercise tags compare equal.
    friend bool operator==(const AmericanExercise&, const AmericanExercise&) = default;
};

/// Exercise styles whose semantics are defined by the core.
template <typename Value>
concept OptionExercise = std::same_as<std::remove_cvref_t<Value>, EuropeanExercise> ||
                         std::same_as<std::remove_cvref_t<Value>, AmericanExercise>;

} // namespace kiyosi
