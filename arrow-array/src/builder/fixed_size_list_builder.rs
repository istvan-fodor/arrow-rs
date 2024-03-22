// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

use crate::builder::ArrayBuilder;
use crate::{ArrayRef, FixedSizeListArray};
use arrow_buffer::NullBufferBuilder;
use arrow_data::ArrayData;
use arrow_schema::{DataType, Field, FieldRef};
use std::any::Any;
use std::sync::Arc;

///  Builder for [`FixedSizeListArray`]
/// ```
/// use arrow_array::{builder::{Int32Builder, FixedSizeListBuilder}, Array, Int32Array};
/// let values_builder = Int32Builder::new();
/// let mut builder = FixedSizeListBuilder::new(values_builder, 3);
///
/// //  [[0, 1, 2], null, [3, null, 5], [6, 7, null]]
/// builder.values().append_value(0);
/// builder.values().append_value(1);
/// builder.values().append_value(2);
/// builder.append(true);
/// builder.values().append_null();
/// builder.values().append_null();
/// builder.values().append_null();
/// builder.append(false);
/// builder.values().append_value(3);
/// builder.values().append_null();
/// builder.values().append_value(5);
/// builder.append(true);
/// builder.values().append_value(6);
/// builder.values().append_value(7);
/// builder.values().append_null();
/// builder.append(true);
/// let list_array = builder.finish();
/// assert_eq!(
///     *list_array.value(0),
///     Int32Array::from(vec![Some(0), Some(1), Some(2)])
/// );
/// assert!(list_array.is_null(1));
/// assert_eq!(
///     *list_array.value(2),
///     Int32Array::from(vec![Some(3), None, Some(5)])
/// );
/// assert_eq!(
///     *list_array.value(3),
///     Int32Array::from(vec![Some(6), Some(7), None])
/// )
/// ```
///
#[derive(Debug)]
pub struct FixedSizeListBuilder<T: ArrayBuilder> {
    null_buffer_builder: NullBufferBuilder,
    values_builder: T,
    list_len: i32,
    field: Option<FieldRef>,
}

impl<T: ArrayBuilder> FixedSizeListBuilder<T> {
    /// Creates a new [`FixedSizeListBuilder`] from a given values array builder
    /// `value_length` is the number of values within each array
    pub fn new(values_builder: T, value_length: i32) -> Self {
        let capacity = values_builder
            .len()
            .checked_div(value_length as _)
            .unwrap_or_default();

        Self::with_capacity(values_builder, value_length, capacity)
    }

    /// Creates a new [`FixedSizeListBuilder`] from a given values array builder
    /// `value_length` is the number of values within each array
    /// `capacity` is the number of items to pre-allocate space for in this builder
    pub fn with_capacity(values_builder: T, value_length: i32, capacity: usize) -> Self {
        Self {
            null_buffer_builder: NullBufferBuilder::new(capacity),
            values_builder,
            list_len: value_length,
            field: None,
        }
    }

    /// Override the field passed to [`ArrayData::builder`]
    ///
    /// By default a nullable field is created with the name `item`
    ///
    /// Note: [`Self::finish`] and [`Self::finish_cloned`] will panic if the
    /// field's data type does not match that of `T`
    pub fn with_field(self, field: impl Into<FieldRef>) -> Self {
        Self {
            field: Some(field.into()),
            ..self
        }
    }
}

impl<T: ArrayBuilder> ArrayBuilder for FixedSizeListBuilder<T>
where
    T: 'static,
{
    /// Returns the builder as a non-mutable `Any` reference.
    fn as_any(&self) -> &dyn Any {
        self
    }

    /// Returns the builder as a mutable `Any` reference.
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Returns the boxed builder as a box of `Any`.
    fn into_box_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }

    /// Returns the number of array slots in the builder
    fn len(&self) -> usize {
        self.null_buffer_builder.len()
    }

    /// Builds the array and reset this builder.
    fn finish(&mut self) -> ArrayRef {
        Arc::new(self.finish())
    }

    /// Builds the array without resetting the builder.
    fn finish_cloned(&self) -> ArrayRef {
        Arc::new(self.finish_cloned())
    }
}

impl<T: ArrayBuilder> FixedSizeListBuilder<T>
where
    T: 'static,
{
    /// Returns the child array builder as a mutable reference.
    ///
    /// This mutable reference can be used to append values into the child array builder,
    /// but you must call [`append`](#method.append) to delimit each distinct list value.
    pub fn values(&mut self) -> &mut T {
        &mut self.values_builder
    }

    /// Returns the length of the list
    pub fn value_length(&self) -> i32 {
        self.list_len
    }

    /// Finish the current fixed-length list array slot
    #[inline]
    pub fn append(&mut self, is_valid: bool) {
        self.null_buffer_builder.append(is_valid);
    }

    /// Builds the [`FixedSizeListBuilder`] and reset this builder.
    pub fn finish(&mut self) -> FixedSizeListArray {
        let len = self.len();
        let values_arr = self.values_builder.finish();
        let values_data = values_arr.to_data();

        assert_eq!(
            values_data.len(), len * self.list_len as usize,
            "Length of the child array ({}) must be the multiple of the value length ({}) and the array length ({}).",
            values_data.len(),
            self.list_len,
            len,
        );

        let nulls = self.null_buffer_builder.finish();

        let field = match &self.field {
            Some(f) => {
                assert_eq!(
                    f.data_type(),
                    values_data.data_type(),
                    "DataType of field ({}) should be the same as the values_builder DataType ({})",
                    f.data_type(),
                    values_data.data_type()
                );
                if !f.is_nullable() {
                    assert!(
                        values_data.null_count() == 0,
                        "field is nullable = false, but the values_builder contains null values"
                    )
                }
                f.clone()
            }
            None => Arc::new(Field::new("item", values_data.data_type().clone(), true)),
        };

        let array_data = ArrayData::builder(DataType::FixedSizeList(field, self.list_len))
            .len(len)
            .add_child_data(values_data)
            .nulls(nulls);

        let array_data = unsafe { array_data.build_unchecked() };

        FixedSizeListArray::from(array_data)
    }

    /// Builds the [`FixedSizeListBuilder`] without resetting the builder.
    pub fn finish_cloned(&self) -> FixedSizeListArray {
        let len = self.len();
        let values_arr = self.values_builder.finish_cloned();
        let values_data = values_arr.to_data();

        assert_eq!(
            values_data.len(), len * self.list_len as usize,
            "Length of the child array ({}) must be the multiple of the value length ({}) and the array length ({}).",
            values_data.len(),
            self.list_len,
            len,
        );

        let nulls = self.null_buffer_builder.finish_cloned();

        let field = match &self.field {
            Some(f) => {
                assert_eq!(
                    f.data_type(),
                    values_data.data_type(),
                    "DataType of field ({}) should be the same as the values_builder DataType ({})",
                    f.data_type(),
                    values_data.data_type()
                );
                if !f.is_nullable() {
                    assert!(
                        values_data.null_count() == 0,
                        "field is nullable = false, but the values_builder contains null values"
                    )
                }
                f.clone()
            }
            None => Arc::new(Field::new("item", values_data.data_type().clone(), true)),
        };

        let array_data = ArrayData::builder(DataType::FixedSizeList(field, self.list_len))
            .len(len)
            .add_child_data(values_data)
            .nulls(nulls);

        let array_data = unsafe { array_data.build_unchecked() };

        FixedSizeListArray::from(array_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::builder::Int32Builder;
    use crate::Array;
    use crate::Int32Array;

    #[test]
    fn test_fixed_size_list_array_builder() {
        let values_builder = Int32Builder::new();
        let mut builder = FixedSizeListBuilder::new(values_builder, 3);

        //  [[0, 1, 2], null, [3, null, 5], [6, 7, null]]
        builder.values().append_value(0);
        builder.values().append_value(1);
        builder.values().append_value(2);
        builder.append(true);
        builder.values().append_null();
        builder.values().append_null();
        builder.values().append_null();
        builder.append(false);
        builder.values().append_value(3);
        builder.values().append_null();
        builder.values().append_value(5);
        builder.append(true);
        builder.values().append_value(6);
        builder.values().append_value(7);
        builder.values().append_null();
        builder.append(true);
        let list_array = builder.finish();

        assert_eq!(DataType::Int32, list_array.value_type());
        assert_eq!(4, list_array.len());
        assert_eq!(1, list_array.null_count());
        assert_eq!(6, list_array.value_offset(2));
        assert_eq!(3, list_array.value_length());
    }

    #[test]
    fn test_fixed_size_list_array_builder_with_field() {
        let values_builder = Int32Builder::new();
        let mut builder = FixedSizeListBuilder::new(values_builder, 3).with_field(Field::new(
            "list_element",
            DataType::Int32,
            true,
        ));

        //  [[0, 1, 2], null, [3, null, 5], [6, 7, null]]
        builder.values().append_value(0);
        builder.values().append_value(1);
        builder.values().append_value(2);
        builder.append(true);
        builder.values().append_null();
        builder.values().append_null();
        builder.values().append_null();
        builder.append(false);
        builder.values().append_value(3);
        builder.values().append_null();
        builder.values().append_value(5);
        builder.append(true);
        builder.values().append_value(6);
        builder.values().append_value(7);
        builder.values().append_null();
        builder.append(true);
        let list_array = builder.finish();

        assert_eq!(DataType::Int32, list_array.value_type());
        assert_eq!(4, list_array.len());
        assert_eq!(1, list_array.null_count());
        assert_eq!(6, list_array.value_offset(2));
        assert_eq!(3, list_array.value_length());
    }

    #[test]
    #[should_panic(
        expected = "field is nullable = false, but the values_builder contains null values"
    )]
    fn test_fixed_size_list_array_builder_with_field_null_panic() {
        let values_builder = Int32Builder::new();
        let builder = FixedSizeListBuilder::new(values_builder, 3);
        let mut builder = builder.with_field(Field::new("list_item", DataType::Int32, false));

        //  [[0, 1, 2], null, [3, null, 5], [6, 7, null]]
        builder.values().append_value(0);
        builder.values().append_value(1);
        builder.values().append_value(2);
        builder.append(true);
        builder.values().append_null();
        builder.values().append_null();
        builder.values().append_null();
        builder.append(false);
        builder.values().append_value(3);
        builder.values().append_null();
        builder.values().append_value(5);
        builder.append(true);

        builder.finish();
    }

    #[test]
    #[should_panic(
        expected = "DataType of field (Int64) should be the same as the values_builder DataType (Int32)"
    )]
    fn test_fixed_size_list_array_builder_with_field_type_panic() {
        let values_builder = Int32Builder::new();
        let builder = FixedSizeListBuilder::new(values_builder, 3);
        let mut builder = builder.with_field(Field::new("list_item", DataType::Int64, true));

        //  [[0, 1, 2], null, [3, null, 5], [6, 7, null]]
        builder.values().append_value(0);
        builder.values().append_value(1);
        builder.values().append_value(2);
        builder.append(true);
        builder.values().append_null();
        builder.values().append_null();
        builder.values().append_null();
        builder.append(false);
        builder.values().append_value(3);
        builder.values().append_value(4);
        builder.values().append_value(5);
        builder.append(true);

        builder.finish();
    }

    #[test]
    fn test_fixed_size_list_array_builder_cloned_with_field() {
        let values_builder = Int32Builder::new();
        let mut builder = FixedSizeListBuilder::new(values_builder, 3).with_field(Field::new(
            "list_element",
            DataType::Int32,
            true,
        ));

        //  [[0, 1, 2], null, [3, null, 5], [6, 7, null]]
        builder.values().append_value(0);
        builder.values().append_value(1);
        builder.values().append_value(2);
        builder.append(true);
        builder.values().append_null();
        builder.values().append_null();
        builder.values().append_null();
        builder.append(false);
        builder.values().append_value(3);
        builder.values().append_null();
        builder.values().append_value(5);
        builder.append(true);
        builder.values().append_value(6);
        builder.values().append_value(7);
        builder.values().append_null();
        builder.append(true);
        let list_array = builder.finish_cloned();

        assert_eq!(DataType::Int32, list_array.value_type());
        assert_eq!(4, list_array.len());
        assert_eq!(1, list_array.null_count());
        assert_eq!(6, list_array.value_offset(2));
        assert_eq!(3, list_array.value_length());
    }

    #[test]
    #[should_panic(
        expected = "field is nullable = false, but the values_builder contains null values"
    )]
    fn test_fixed_size_list_array_builder_cloned_with_field_null_panic() {
        let values_builder = Int32Builder::new();
        let builder = FixedSizeListBuilder::new(values_builder, 3);
        let mut builder = builder.with_field(Field::new("list_item", DataType::Int32, false));

        //  [[0, 1, 2], null, [3, null, 5], [6, 7, null]]
        builder.values().append_value(0);
        builder.values().append_value(1);
        builder.values().append_value(2);
        builder.append(true);
        builder.values().append_null();
        builder.values().append_null();
        builder.values().append_null();
        builder.append(false);
        builder.values().append_value(3);
        builder.values().append_null();
        builder.values().append_value(5);
        builder.append(true);

        builder.finish_cloned();
    }

    #[test]
    #[should_panic(
        expected = "DataType of field (Int64) should be the same as the values_builder DataType (Int32)"
    )]
    fn test_fixed_size_list_array_builder_cloned_with_field_type_panic() {
        let values_builder = Int32Builder::new();
        let builder = FixedSizeListBuilder::new(values_builder, 3);
        let mut builder = builder.with_field(Field::new("list_item", DataType::Int64, true));

        //  [[0, 1, 2], null, [3, null, 5], [6, 7, null]]
        builder.values().append_value(0);
        builder.values().append_value(1);
        builder.values().append_value(2);
        builder.append(true);
        builder.values().append_null();
        builder.values().append_null();
        builder.values().append_null();
        builder.append(false);
        builder.values().append_value(3);
        builder.values().append_value(4);
        builder.values().append_value(5);
        builder.append(true);

        builder.finish_cloned();
    }

    #[test]
    fn test_fixed_size_list_array_builder_finish_cloned() {
        let values_builder = Int32Builder::new();
        let mut builder = FixedSizeListBuilder::new(values_builder, 3);

        //  [[0, 1, 2], null, [3, null, 5], [6, 7, null]]
        builder.values().append_value(0);
        builder.values().append_value(1);
        builder.values().append_value(2);
        builder.append(true);
        builder.values().append_null();
        builder.values().append_null();
        builder.values().append_null();
        builder.append(false);
        builder.values().append_value(3);
        builder.values().append_null();
        builder.values().append_value(5);
        builder.append(true);
        let mut list_array = builder.finish_cloned();

        assert_eq!(DataType::Int32, list_array.value_type());
        assert_eq!(3, list_array.len());
        assert_eq!(1, list_array.null_count());
        assert_eq!(3, list_array.value_length());

        builder.values().append_value(6);
        builder.values().append_value(7);
        builder.values().append_null();
        builder.append(true);
        builder.values().append_null();
        builder.values().append_null();
        builder.values().append_null();
        builder.append(false);
        list_array = builder.finish();

        assert_eq!(DataType::Int32, list_array.value_type());
        assert_eq!(5, list_array.len());
        assert_eq!(2, list_array.null_count());
        assert_eq!(6, list_array.value_offset(2));
        assert_eq!(3, list_array.value_length());
    }

    #[test]
    fn test_fixed_size_list_array_builder_empty() {
        let values_builder = Int32Array::builder(5);
        let mut builder = FixedSizeListBuilder::new(values_builder, 3);
        assert!(builder.is_empty());
        let arr = builder.finish();
        assert_eq!(0, arr.len());
        assert_eq!(0, builder.len());
    }

    #[test]
    fn test_fixed_size_list_array_builder_finish() {
        let values_builder = Int32Array::builder(5);
        let mut builder = FixedSizeListBuilder::new(values_builder, 3);

        builder.values().append_slice(&[1, 2, 3]);
        builder.append(true);
        builder.values().append_slice(&[4, 5, 6]);
        builder.append(true);

        let mut arr = builder.finish();
        assert_eq!(2, arr.len());
        assert_eq!(0, builder.len());

        builder.values().append_slice(&[7, 8, 9]);
        builder.append(true);
        arr = builder.finish();
        assert_eq!(1, arr.len());
        assert_eq!(0, builder.len());
    }

    #[test]
    #[should_panic(
        expected = "Length of the child array (10) must be the multiple of the value length (3) and the array length (3)."
    )]
    fn test_fixed_size_list_array_builder_fail() {
        let values_builder = Int32Array::builder(5);
        let mut builder = FixedSizeListBuilder::new(values_builder, 3);

        builder.values().append_slice(&[1, 2, 3]);
        builder.append(true);
        builder.values().append_slice(&[4, 5, 6]);
        builder.append(true);
        builder.values().append_slice(&[7, 8, 9, 10]);
        builder.append(true);

        builder.finish();
    }
}
