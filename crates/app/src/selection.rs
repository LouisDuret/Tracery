//! Sélection de formes (ADR-27) : une forme principale, dont le panneau de
//! droite montre les réglages et le canevas les poignées, et des formes
//! ajoutées (Maj ou Ctrl + clic, rectangle de sélection).

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    /// Forme principale.
    pub primary: Option<u64>,
    /// Autres formes, dans l'ordre d'ajout (sans la principale).
    pub more: Vec<u64>,
}

impl Selection {
    /// Sélectionne une seule forme (ou rien).
    pub fn set(&mut self, id: Option<u64>) {
        self.primary = id;
        self.more.clear();
    }
    /// Sélectionne une liste de formes ; la première devient principale.
    pub fn set_many(&mut self, ids: &[u64]) {
        self.set(None);
        for &id in ids {
            self.add(id);
        }
    }
    /// Toutes les formes sélectionnées, principale en tête.
    pub fn ids(&self) -> Vec<u64> {
        self.primary
            .iter()
            .chain(self.more.iter())
            .copied()
            .collect()
    }
    pub fn contains(&self, id: u64) -> bool {
        self.primary == Some(id) || self.more.contains(&id)
    }
    pub fn len(&self) -> usize {
        self.primary.iter().count() + self.more.len()
    }
    pub fn is_multi(&self) -> bool {
        self.len() > 1
    }
    /// Ajoute une forme (principale si rien n'est sélectionné).
    pub fn add(&mut self, id: u64) {
        if self.contains(id) {
            return;
        }
        if self.primary.is_none() {
            self.primary = Some(id);
        } else {
            self.more.push(id);
        }
    }
    /// Retire une forme ; si c'était la principale, la suivante la remplace.
    pub fn remove(&mut self, id: u64) {
        if self.primary == Some(id) {
            self.primary = if self.more.is_empty() {
                None
            } else {
                Some(self.more.remove(0))
            };
        } else {
            self.more.retain(|&x| x != id);
        }
    }
    /// Ajoute ou retire (Maj / Ctrl + clic).
    pub fn toggle(&mut self, id: u64) {
        if self.contains(id) {
            self.remove(id);
        } else {
            self.add(id);
        }
    }
    /// Ne garde que les formes qui vérifient `keep` (formes encore présentes).
    pub fn retain(&mut self, keep: impl Fn(u64) -> bool) {
        for id in self.ids() {
            if !keep(id) {
                self.remove(id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ajout_retrait() {
        let mut s = Selection::default();
        s.set(Some(1));
        s.toggle(2);
        s.toggle(3);
        assert_eq!(s.ids(), vec![1, 2, 3]);
        assert!(s.is_multi());
        // retirer la principale promeut la suivante
        s.toggle(1);
        assert_eq!(s.primary, Some(2));
        assert_eq!(s.ids(), vec![2, 3]);
        s.toggle(3);
        assert!(!s.is_multi());
        s.toggle(2);
        assert_eq!(s.len(), 0);
        // depuis le vide, l'ajout devient principal
        s.toggle(7);
        assert_eq!(s.primary, Some(7));
    }

    #[test]
    fn liste_et_validation() {
        let mut s = Selection::default();
        s.set_many(&[4, 5, 4, 6]);
        assert_eq!(s.ids(), vec![4, 5, 6]);
        s.retain(|id| id != 4);
        assert_eq!(s.ids(), vec![5, 6]);
        s.set(Some(9));
        assert_eq!(s.ids(), vec![9]);
        s.set_many(&[]);
        assert_eq!(s.primary, None);
    }
}
