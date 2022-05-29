/// A Vec of Vecs that preserves allocation of the inner vecs; clearing the inner elements without ever wiping away the vecs
pub struct MultiVec<T> {
   index: usize,
   inner: Vec<Vec<T>>,
}

impl<T> MultiVec<T> {
   pub fn new() -> MultiVec<T> {
      MultiVec {
         index: 0,
         inner: Vec::new(),
      }
   }

   pub fn reset(&mut self) {
      self.index = 0;
      for vec in self.inner.iter_mut() {
         vec.clear()
      }
   }

   pub fn add_items(&mut self, items: &[T])
   where
      T: Clone,
   {
      while self.index >= self.inner.len() {
         self.inner.push(Vec::new());
      }
      self.inner[self.index].extend_from_slice(items);
      self.index += 1;
   }

   pub fn contains_items(&self, items: &[T]) -> bool
   where
      T: PartialEq,
   {
      for vec in self.inner.iter() {
         if vec.as_slice() == items {
            return true;
         }
      }
      false
   }

   pub fn push_to_last_bucket(&mut self, item: T) {
      while self.index >= self.inner.len() {
         self.inner.push(Vec::new());
      }
      self.inner[self.index].push(item);
   }

   pub fn finalize_last_bucket(&mut self) {
      self.index += 1;
   }

   pub fn get_valid_inner(&self) -> &[Vec<T>] {
      &self.inner[0..self.index]
   }

   pub fn len(&self) -> usize {
      self.index
   }
}
