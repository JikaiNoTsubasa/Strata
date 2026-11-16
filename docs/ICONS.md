# Icônes de Strata — où les placer

> L'icône actuelle représente une **carotte géologique** : un échantillon cylindrique (clin d'œil au symbole « base de données ») dont le flanc montre des strates aux limites ondulées, du sable au bleu profond. La version symbolique reprend le cylindre découpé en couches ondulées. Elle peut être remplacée à tout moment en écrasant les fichiers ci-dessous. Ce document décrit le format attendu et les emplacements dans le dépôt.
> `<APP_ID>` = identifiant d'application = `fr.coregeek.Strata` (cf. ADR-0016).

## 1. Fichiers à fournir

| Fichier | Emplacement dans le dépôt | Obligatoire |
|---|---|---|
| Icône de l'application (couleur) | `data/icons/hicolor/scalable/apps/<APP_ID>.svg` | **Oui** |
| Icône symbolique (monochrome) | `data/icons/hicolor/symbolic/apps/<APP_ID>-symbolic.svg` | **Oui** (GNOME l'utilise dans le panneau, les notifications, la vue d'ensemble à petite taille) |
| Variante de développement (optionnel) | `data/icons/hicolor/scalable/apps/<APP_ID>.Devel.svg` | Non — permet de distinguer une build de dev (ex. bandeau rayé) |

Uniquement du **SVG** : les tailles PNG sont inutiles avec GTK4 (rendu vectoriel). Le nom de fichier doit être **exactement** l'identifiant d'application, sinon le bureau ne trouvera pas l'icône.

## 2. Contraintes de format (GNOME HIG)

### Icône couleur
- Canevas **128 × 128 px** (`viewBox="0 0 128 128"`).
- Style GNOME : formes simples, légère perspective, palette de la [GNOME HIG](https://developer.gnome.org/hig/guidelines/app-icons.html).
- Lisible à 32 px ; tester à 16, 32, 64 et 128 px.
- SVG **simple** : pas de texte (convertir en chemins), pas d'images bitmap intégrées, pas de filtres complexes ni de dépendance à une police. Nettoyer les métadonnées de l'éditeur (Inkscape : *Enregistrer sous → SVG optimisé*, ou `scour`).

### Icône symbolique
- Canevas **16 × 16 px** (`viewBox="0 0 16 16"`).
- **Une seule couleur pleine** (`#2e3436` ou `currentColor`) ; GTK la recolore selon le thème clair/sombre.
- Pas de dégradé, pas de transparence partielle, traits ≥ 1 px alignés sur la grille.

### Outils conseillés
- [App Icon Preview](https://flathub.org/apps/org.gnome.design.AppIconPreview) : prévisualisation dans le contexte GNOME et export aux bons formats.
- [Icon Library](https://flathub.org/apps/org.gnome.design.IconLibrary) : icônes symboliques existantes pour inspiration.

## 3. Ce qui est fait automatiquement

Une fois les fichiers en place, rien d'autre à modifier :
- **Installation** : Meson/Flatpak copient `data/icons/hicolor/` vers `$prefix/share/icons/hicolor/` (puis `gtk-update-icon-cache`).
- **`.desktop`** : `Icon=<APP_ID>`.
- **AppStream** (`data/<APP_ID>.metainfo.xml`) : référence l'icône par l'identifiant ; Flathub la valide.
- **Fenêtre « À propos »** : `application-icon` = `<APP_ID>`.
- **Développement (`cargo run`)** : le binaire ajoute `data/icons` au chemin de recherche du thème d'icônes, l'icône apparaît sans installation.

Si un fichier d'icône manque, GTK affiche une icône générique ; Flathub refuse la publication sans icône.

## 4. Captures d'écran (pour Flathub, plus tard)
`data/screenshots/*.png`, référencées dans l'AppStream ; Flathub les exige pour la publication (J8).
