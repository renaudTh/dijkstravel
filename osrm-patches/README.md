# Correctifs du graphe OSRM des DOM

Ces fichiers OSM sont fusionnés dans l'extrait `dom-routes` avant `osrm-extract` (voir `docker-compose.yml`).
Les scripts qui les ont produits ont été jetés. Les fichiers versionnés sont donc la référence.

- `debarcaderes.osm.pbf` relie à la route les bouts de ferry qui aboutissent sur un quai piéton (footway, steps…).
  Le profil car peut ainsi débarquer aux Saintes, à la Désirade, etc.
- `pirogues.osm.pbf` contient les lignes de pirogue de Guyane (Maroni et Lawa, Oyapock, rivière Ouanary), absentes d'OSM.
  Chaque ligne suit le lit du fleuve d'un débarcadère à l'autre, et chaque débarcadère est relié à la route du bourg.
  Saül et Saint-Élie ne sont desservis ni par la route ni par le fleuve.

Données © les contributeurs d'OpenStreetMap, ODbL.
