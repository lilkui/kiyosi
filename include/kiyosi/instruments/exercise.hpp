#pragma once

#include <concepts>
#include <type_traits>

namespace kiyosi {

/// Requirement for a value type used as an option exercise-style tag.
template <typename Value>
concept OptionExercise = std::copy_constructible<std::remove_cvref_t<Value>> &&
                         std::equality_comparable<std::remove_cvref_t<Value>>;

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

} // namespace kiyosi
