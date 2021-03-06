use descartes::{LinePath, WithUniqueOrthogonal, RoughEq};
use compact::CVec;
use kay::{ActorSystem, World, Actor, TypedID};
use monet::{Instance, Vertex, Mesh, Renderer, RendererID};
use super::lane::{Lane, LaneID, SwitchLane, SwitchLaneID};
use render_layers::RenderLayers;

use style::colors;
use style::dimensions::{LANE_DISTANCE, LANE_WIDTH, LANE_MARKER_WIDTH, LANE_MARKER_DASH_GAP,
LANE_MARKER_DASH_LENGTH};

use itertools::Itertools;

#[path = "./resources/car.rs"]
mod car;

#[path = "./resources/traffic_light.rs"]
mod traffic_light;

use monet::{Renderable, RenderableID, GrouperID, GrouperIndividual, GrouperIndividualID};

impl Lane {
    fn car_instances(&self) -> CVec<Instance> {
        let mut cars_iter = self.microtraffic.cars.iter();
        let mut car_instances = CVec::with_capacity(self.microtraffic.cars.len());
        for (segment, distance_pair) in self.construction.path.segments_with_distances() {
            for car in
                cars_iter.take_while_ref(|car| *car.position - distance_pair[0] < segment.length())
            {
                let position2d = segment.along(*car.position - distance_pair[0]);
                let direction = segment.direction();
                car_instances.push(Instance {
                    instance_position: [position2d.x, position2d.y, 0.0],
                    instance_direction: [direction.x, direction.y],
                    instance_color: if DEBUG_VIEW_LANDMARKS {
                        colors::RANDOM_COLORS[car.destination.landmark.as_raw().instance_id as usize
                                                  % colors::RANDOM_COLORS.len()]
                    } else {
                        colors::RANDOM_COLORS
                            [car.trip.as_raw().instance_id as usize % colors::RANDOM_COLORS.len()]
                    },
                })
            }
        }

        car_instances
    }

    pub fn get_car_instances(&self, ui: BrowserUIID, world: &mut World) {
        ui.on_car_instances(self.id.as_raw(), self.car_instances(), world);
    }
}

impl Renderable for Lane {
    #[cfg_attr(feature = "cargo-clippy", allow(cyclomatic_complexity))]
    fn render(&mut self, renderer_id: RendererID, frame: usize, world: &mut World) {
        let mut car_instances = self.car_instances();

        if DEBUG_VIEW_OBSTACLES {
            for &(obstacle, _id) in &self.microtraffic.obstacles {
                let position2d = if *obstacle.position < self.construction.length {
                    self.construction.path.along(*obstacle.position)
                } else {
                    self.construction.path.end()
                        + (*obstacle.position - self.construction.length)
                            * self.construction.path.end_direction()
                };
                let direction = self.construction.path.direction_along(*obstacle.position);

                car_instances.push(Instance {
                    instance_position: [position2d.x, position2d.y, 0.0],
                    instance_direction: [direction.x, direction.y],
                    instance_color: [1.0, 0.0, 0.0],
                });
            }
        }

        if !car_instances.is_empty() {
            renderer_id.add_several_instances(
                RenderLayers::Car as u32,
                frame,
                car_instances,
                world,
            );
        }
        // no traffic light for u-turn
        if self.connectivity.on_intersection
            && !self
                .construction
                .path
                .end_direction()
                .rough_eq_by(-self.construction.path.start_direction(), 0.1)
        {
            let mut position = self.construction.path.start();
            let (position_shift, batch_id) = if !self
                .construction
                .path
                .start_direction()
                .rough_eq_by(self.construction.path.end_direction(), 0.5)
            {
                let dot = self
                    .construction
                    .path
                    .end_direction()
                    .dot(&self.construction.path.start_direction().orthogonal());
                let shift = if dot > 0.0 { 1.0 } else { -1.0 };
                let batch_id = if dot > 0.0 {
                    RenderLayers::TrafficLightLightRight as u32
                } else {
                    RenderLayers::TrafficLightLightLeft as u32
                };
                (shift, batch_id)
            } else {
                (0.0, RenderLayers::TrafficLightLight as u32)
            };
            position += self.construction.path.start_direction().orthogonal() * position_shift;
            let direction = self.construction.path.start_direction();

            let instance = Instance {
                instance_position: [position.x, position.y, 6.0],
                instance_direction: [direction.x, direction.y],
                instance_color: [0.1, 0.1, 0.1],
            };
            renderer_id.add_instance(RenderLayers::TrafficLightBox as u32, frame, instance, world);

            if self.microtraffic.yellow_to_red && self.microtraffic.green {
                let instance = Instance {
                    instance_position: [position.x, position.y, 6.7],
                    instance_direction: [direction.x, direction.y],
                    instance_color: [1.0, 0.8, 0.0],
                };
                renderer_id.add_instance(batch_id, frame, instance, world)
            } else if self.microtraffic.green {
                let instance = Instance {
                    instance_position: [position.x, position.y, 6.1],
                    instance_direction: [direction.x, direction.y],
                    instance_color: [0.0, 1.0, 0.2],
                };
                renderer_id.add_instance(batch_id, frame, instance, world)
            }

            if !self.microtraffic.green {
                let instance = Instance {
                    instance_position: [position.x, position.y, 7.3],
                    instance_direction: [direction.x, direction.y],
                    instance_color: [1.0, 0.0, 0.0],
                };
                renderer_id.add_instance(batch_id, frame, instance, world);

                if self.microtraffic.yellow_to_green {
                    let instance = Instance {
                        instance_position: [position.x, position.y, 6.7],
                        instance_direction: [direction.x, direction.y],
                        instance_color: [1.0, 0.8, 0.0],
                    };
                    renderer_id.add_instance(batch_id, frame, instance, world)
                }
            }
        }

        if DEBUG_VIEW_SIGNALS && self.connectivity.on_intersection {
            let mesh = Mesh::from_path_as_band(
                &self.construction.path,
                0.3,
                if self.microtraffic.green { 0.4 } else { 0.2 },
            );
            let instance = Instance::with_color(if self.microtraffic.green {
                [0.0, 1.0, 0.0]
            } else {
                [1.0, 0.0, 0.0]
            });
            renderer_id.update_individual(
                RenderLayers::DebugSignalState as u32 + self.id.as_raw().instance_id as u32,
                mesh,
                instance,
                true,
                world,
            );
        }

        // let has_next = self.connectivity.interactions.iter().any(|inter| {
        //     match inter.kind {
        //         InteractionKind::Next { .. } => true,
        //         _ => false,
        //     }
        // });
        // if !has_next {
        //     let instance = Instance {
        //         instance_position: [
        //             self.construction.path.end().x,
        //             self.construction.path.end().y,
        //             0.5,
        //         ],
        //         instance_direction: [1.0, 0.0],
        //         instance_color: [1.0, 0.0, 0.0],
        //     };
        //     renderer_id.add_instance( RenderLayers::DebugConnectivity as u32,
        //                               frame,
        //                               instance,
        //                               world);
        // }

        // let has_previous = self.connectivity.interactions.iter().any(|inter| {
        //     match inter.kind {
        //         InteractionKind::Previous { .. } => true,
        //         _ => false,
        //     }
        // });
        // if !has_previous {
        //     let instance = Instance {
        //         instance_position: [
        //             self.construction.path.start().x,
        //             self.construction.path.start().y,
        //             0.5,
        //         ],
        //         instance_direction: [1.0, 0.0],
        //         instance_color: [0.0, 1.0, 0.0],
        //     };
        //     renderer_id.add_instance( RenderLayers::DebugConnectivity as u32,
        //                               frame,
        //                               instance,
        //                               world);
        // }

        if DEBUG_VIEW_LANDMARKS && self.pathfinding.routes_changed {
            let (random_color, is_landmark) = if let Some(location) = self.pathfinding.location {
                let random_color: [f32; 3] = colors::RANDOM_COLORS
                    [location.landmark.as_raw().instance_id as usize % colors::RANDOM_COLORS.len()];
                let weaker_random_color = [
                    (random_color[0] + 1.0) / 2.0,
                    (random_color[1] + 1.0) / 2.0,
                    (random_color[2] + 1.0) / 2.0,
                ];
                (weaker_random_color, location.is_landmark())
            } else {
                ([1.0, 1.0, 1.0], false)
            };

            let instance = Mesh::from_path_as_band(
                &self.construction.path,
                if is_landmark { 2.5 } else { 1.0 },
                0.4,
            );
            renderer_id.update_individual(
                RenderLayers::DebugLandmarkAssociation as u32 + self.id.as_raw().instance_id as u32,
                instance,
                Instance::with_color(random_color),
                true,
                world,
            );
        }

        use super::pathfinding::DEBUG_VIEW_CONNECTIVITY;

        if DEBUG_VIEW_CONNECTIVITY {
            if !self.pathfinding.debug_highlight_for.is_empty() {
                let (random_color, is_landmark) = if let Some(location) = self.pathfinding.location
                {
                    let random_color: [f32; 3] =
                        colors::RANDOM_COLORS[location.landmark.as_raw().instance_id as usize
                                                  % colors::RANDOM_COLORS.len()];
                    (random_color, location.is_landmark())
                } else {
                    ([1.0, 1.0, 1.0], false)
                };

                let mesh = Mesh::from_path_as_band(
                    &self.construction.path,
                    if is_landmark { 2.5 } else { 1.0 },
                    0.4,
                );
                renderer_id.update_individual(
                    RenderLayers::DebugConnectivity as u32 + self.id.as_raw().instance_id as u32,
                    mesh,
                    Instance::with_color(random_color),
                    true,
                    world,
                );
            } else {
                renderer_id.update_individual(
                    RenderLayers::DebugConnectivity as u32 + self.id.as_raw().instance_id as u32,
                    Mesh::empty(),
                    Instance::with_color([0.0, 0.0, 0.0]),
                    true,
                    world,
                );
            }
        }
    }
}

pub fn lane_mesh(path: &LinePath) -> Mesh {
    Mesh::from_path_as_band(path, LANE_WIDTH, 0.0)
}

pub fn marker_mesh(path: &LinePath) -> (Mesh, Mesh) {
    // use negative widths to simulate a shifted band on each side
    (
        Mesh::from_path_as_band_asymmetric(
            &path,
            LANE_DISTANCE / 2.0 + LANE_MARKER_WIDTH / 2.0,
            -(LANE_DISTANCE / 2.0 - LANE_MARKER_WIDTH / 2.0),
            0.1,
        ),
        Mesh::from_path_as_band_asymmetric(
            &path,
            -(LANE_DISTANCE / 2.0 - LANE_MARKER_WIDTH / 2.0),
            LANE_DISTANCE / 2.0 + LANE_MARKER_WIDTH / 2.0,
            0.1,
        ),
    )
}

pub fn switch_marker_gap_mesh(path: &LinePath) -> Mesh {
    path.dash(LANE_MARKER_DASH_GAP, LANE_MARKER_DASH_LENGTH)
        .into_iter()
        .filter_map(|maybe_dash| {
            maybe_dash.map(|dash| Mesh::from_path_as_band(&dash, LANE_MARKER_WIDTH * 2.0, 0.0))
        })
        .sum()
}

use browser_ui::BrowserUIID;

impl Lane {
    pub fn get_render_info(&mut self, ui: BrowserUIID, world: &mut World) {
        ui.on_lane_constructed(
            self.id.as_raw(),
            self.construction.path.clone(),
            false,
            self.connectivity.on_intersection,
            world,
        );
    }
}

impl SwitchLane {
    pub fn get_render_info(&mut self, ui: BrowserUIID, world: &mut World) {
        ui.on_lane_constructed(
            self.id.as_raw(),
            self.construction.path.clone(),
            true,
            false,
            world,
        );
    }
}

impl GrouperIndividual for Lane {
    fn render_to_grouper(
        &mut self,
        grouper: GrouperID,
        base_individual_id: u32,
        world: &mut World,
    ) {
        let maybe_path = if self.construction.progress - CONSTRUCTION_ANIMATION_DELAY
            < self.construction.length
        {
            self.construction.path.subsection(
                0.0,
                (self.construction.progress - CONSTRUCTION_ANIMATION_DELAY).max(0.0),
            )
        } else {
            Some(self.construction.path.clone())
        };
        if base_individual_id == RenderLayers::LaneAsphalt as u32 {
            grouper.update(
                self.id_as(),
                maybe_path
                    .map(|path| {
                        Mesh::from_path_as_band(
                            &path,
                            LANE_WIDTH,
                            if self.connectivity.on_intersection {
                                0.2
                            } else {
                                0.0
                            },
                        )
                    })
                    .unwrap_or_else(Mesh::empty),
                world,
            );
            if self.construction.progress - CONSTRUCTION_ANIMATION_DELAY > self.construction.length
            {
                grouper.freeze(self.id_as(), world);
            }
        } else {
            let left_marker = maybe_path
                .clone()
                .and_then(|path| path.shift_orthogonally(LANE_DISTANCE / 2.0))
                .map(|path| Mesh::from_path_as_band(&path, LANE_MARKER_WIDTH, 0.1))
                .unwrap_or_else(Mesh::empty);

            let right_marker = maybe_path
                .and_then(|path| path.shift_orthogonally(-LANE_DISTANCE / 2.0))
                .map(|path| Mesh::from_path_as_band(&path, LANE_MARKER_WIDTH, 0.1))
                .unwrap_or_else(Mesh::empty);
            grouper.update(self.id_as(), left_marker + right_marker, world);
            if self.construction.progress - CONSTRUCTION_ANIMATION_DELAY > self.construction.length
            {
                grouper.freeze(self.id_as(), world);
            }
        }
    }
}

impl SwitchLane {
    fn car_instances(&self) -> CVec<Instance> {
        let mut cars_iter = self.microtraffic.cars.iter();
        let mut car_instances = CVec::with_capacity(self.microtraffic.cars.len());
        for (segment, distance_pair) in self.construction.path.segments_with_distances() {
            for car in
                cars_iter.take_while_ref(|car| *car.position - distance_pair[0] < segment.length())
            {
                let position2d = segment.along(*car.position - distance_pair[0]);
                let direction = segment.direction();
                let rotated_direction =
                    (direction + 0.3 * car.switch_velocity * direction.orthogonal()).normalize();
                let shifted_position2d =
                    position2d + 2.5 * direction.orthogonal() * car.switch_position;
                car_instances.push(Instance {
                    instance_position: [shifted_position2d.x, shifted_position2d.y, 0.0],
                    instance_direction: [rotated_direction.x, rotated_direction.y],
                    instance_color: if DEBUG_VIEW_LANDMARKS {
                        colors::RANDOM_COLORS[car.destination.landmark.as_raw().instance_id as usize
                                                  % colors::RANDOM_COLORS.len()]
                    } else {
                        colors::RANDOM_COLORS
                            [car.trip.as_raw().instance_id as usize % colors::RANDOM_COLORS.len()]
                    },
                })
            }
        }

        car_instances
    }

    pub fn get_car_instances(&mut self, ui: BrowserUIID, world: &mut World) {
        ui.on_car_instances(self.id.as_raw(), self.car_instances(), world);
    }
}

impl Renderable for SwitchLane {
    fn render(&mut self, renderer_id: RendererID, frame: usize, world: &mut World) {
        let mut car_instances = self.car_instances();

        if DEBUG_VIEW_TRANSFER_OBSTACLES {
            for obstacle in &self.microtraffic.left_obstacles {
                let position2d = if *obstacle.position < self.construction.length {
                    self.construction.path.along(*obstacle.position)
                } else {
                    self.construction.path.end()
                        + (*obstacle.position - self.construction.length)
                            * self.construction.path.end_direction()
                }
                    - 1.0 * self
                        .construction
                        .path
                        .direction_along(*obstacle.position)
                        .orthogonal();
                let direction = self.construction.path.direction_along(*obstacle.position);

                car_instances.push(Instance {
                    instance_position: [position2d.x, position2d.y, 0.0],
                    instance_direction: [direction.x, direction.y],
                    instance_color: [1.0, 0.7, 0.7],
                });
            }

            for obstacle in &self.microtraffic.right_obstacles {
                let position2d = if *obstacle.position < self.construction.length {
                    self.construction.path.along(*obstacle.position)
                } else {
                    self.construction.path.end()
                        + (*obstacle.position - self.construction.length)
                            * self.construction.path.end_direction()
                }
                    + 1.0 * self
                        .construction
                        .path
                        .direction_along(*obstacle.position)
                        .orthogonal();
                let direction = self.construction.path.direction_along(*obstacle.position);

                car_instances.push(Instance {
                    instance_position: [position2d.x, position2d.y, 0.0],
                    instance_direction: [direction.x, direction.y],
                    instance_color: [1.0, 0.7, 0.7],
                });
            }
        }

        if !car_instances.is_empty() {
            renderer_id.add_several_instances(
                RenderLayers::Car as u32,
                frame,
                car_instances,
                world,
            );
        }

        if self.connectivity.left.is_none() {
            let position = self.construction.path.along(self.construction.length / 2.0)
                + self
                    .construction
                    .path
                    .direction_along(self.construction.length / 2.0)
                    .orthogonal();
            renderer_id.add_instance(
                RenderLayers::DebugConnectivity as u32,
                frame,
                Instance {
                    instance_position: [position.x, position.y, 0.0],
                    instance_direction: [1.0, 0.0],
                    instance_color: [1.0, 0.0, 0.0],
                },
                world,
            );
        }
        if self.connectivity.right.is_none() {
            let position = self.construction.path.along(self.construction.length / 2.0)
                - self
                    .construction
                    .path
                    .direction_along(self.construction.length / 2.0)
                    .orthogonal();
            renderer_id.add_instance(
                RenderLayers::DebugConnectivity as u32,
                frame,
                Instance {
                    instance_position: [position.x, position.y, 0.0],
                    instance_direction: [1.0, 0.0],
                    instance_color: [1.0, 0.0, 0.0],
                },
                world,
            );
        }
    }
}

impl GrouperIndividual for SwitchLane {
    fn render_to_grouper(
        &mut self,
        grouper: GrouperID,
        _base_individual_id: u32,
        world: &mut World,
    ) {
        let maybe_path = if self.construction.progress - 2.0 * CONSTRUCTION_ANIMATION_DELAY
            < self.construction.length
        {
            self.construction.path.subsection(
                0.0,
                (self.construction.progress - 2.0 * CONSTRUCTION_ANIMATION_DELAY).max(0.0),
            )
        } else {
            Some(self.construction.path.clone())
        };

        grouper.update(
            self.id_as(),
            maybe_path
                .map(|path| {
                    path.dash(LANE_MARKER_DASH_GAP, LANE_MARKER_DASH_LENGTH)
                        .into_iter()
                        .filter_map(|maybe_dash| {
                            maybe_dash.map(|dash| Mesh::from_path_as_band(&dash, 0.8, 0.2))
                        })
                        .sum()
                })
                .unwrap_or_else(Mesh::empty),
            world,
        );
        if self.construction.progress - 2.0 * CONSTRUCTION_ANIMATION_DELAY
            > self.construction.length
        {
            grouper.freeze(self.id_as(), world);
        }
    }
}

pub fn setup(system: &mut ActorSystem) {
    system.register::<LaneRenderer>();
    auto_setup(system);
}

pub fn spawn(world: &mut World) {
    let asphalt_group = GrouperID::spawn(
        colors::ASPHALT,
        RenderLayers::LaneAsphalt as u32,
        false,
        world,
    );

    let marker_group = GrouperID::spawn(
        colors::ROAD_MARKER,
        RenderLayers::LaneMarker as u32,
        true,
        world,
    );

    let gaps_group = GrouperID::spawn(
        colors::ASPHALT,
        RenderLayers::LaneMarkerGaps as u32,
        true,
        world,
    );

    LaneRendererID::spawn(asphalt_group, marker_group, gaps_group, world);
}

const CONSTRUCTION_ANIMATION_DELAY: f32 = 120.0;

const DEBUG_VIEW_LANDMARKS: bool = false;
const DEBUG_VIEW_SIGNALS: bool = false;
const DEBUG_VIEW_OBSTACLES: bool = false;
const DEBUG_VIEW_TRANSFER_OBSTACLES: bool = false;

#[derive(Compact, Clone)]
pub struct LaneRenderer {
    id: LaneRendererID,
    asphalt_grouper: GrouperID,
    marker_grouper: GrouperID,
    gaps_grouper: GrouperID,
}

impl Renderable for LaneRenderer {
    fn init(&mut self, renderer_id: RendererID, world: &mut World) {
        renderer_id.add_batch(RenderLayers::Car as u32, car::create(), world);
        renderer_id.add_batch(
            RenderLayers::TrafficLightBox as u32,
            traffic_light::create(),
            world,
        );
        renderer_id.add_batch(
            RenderLayers::TrafficLightLight as u32,
            traffic_light::create_light(),
            world,
        );
        renderer_id.add_batch(
            RenderLayers::TrafficLightLightLeft as u32,
            traffic_light::create_light_left(),
            world,
        );
        renderer_id.add_batch(
            RenderLayers::TrafficLightLightRight as u32,
            traffic_light::create_light_right(),
            world,
        );

        renderer_id.add_batch(
            RenderLayers::DebugConnectivity as u32,
            Mesh::new(
                vec![
                    Vertex {
                        position: [-1.0, -1.0, 0.0],
                    },
                    Vertex {
                        position: [1.0, -1.0, 0.0],
                    },
                    Vertex {
                        position: [1.0, 1.0, 0.0],
                    },
                    Vertex {
                        position: [-1.0, 1.0, 0.0],
                    },
                ],
                vec![0, 1, 2, 2, 3, 0],
            ),
            world,
        );
    }

    fn render(&mut self, renderer_id: RendererID, frame: usize, world: &mut World) {
        // Render a single invisible car to clean all instances every frame
        renderer_id.add_instance(
            RenderLayers::Car as u32,
            frame,
            Instance {
                instance_position: [-1_000_000.0, -1_000_000.0, -1_000_000.0],
                instance_direction: [0.0, 0.0],
                instance_color: [0.0, 0.0, 0.0],
            },
            world,
        );

        let lanes_as_renderables: RenderableID = Lane::local_broadcast(world).into();
        lanes_as_renderables.render(renderer_id, frame, world);

        let switch_lanes_as_renderables: RenderableID = SwitchLane::local_broadcast(world).into();
        switch_lanes_as_renderables.render(renderer_id, frame, world);
    }
}

impl LaneRenderer {
    pub fn spawn(
        id: LaneRendererID,
        asphalt_grouper: GrouperID,
        marker_grouper: GrouperID,
        gaps_grouper: GrouperID,
        _: &mut World,
    ) -> LaneRenderer {
        LaneRenderer {
            id,
            asphalt_grouper,
            marker_grouper,
            gaps_grouper,
        }
    }

    pub fn on_build(
        &mut self,
        lane: GrouperIndividualID,
        on_intersection: bool,
        world: &mut World,
    ) {
        self.asphalt_grouper.initial_add(lane, world);

        if !on_intersection {
            self.marker_grouper.initial_add(lane, world);
        }
    }

    pub fn on_build_switch(&mut self, lane: GrouperIndividualID, world: &mut World) {
        self.gaps_grouper.initial_add(lane, world);
    }

    pub fn on_unbuild(
        &mut self,
        lane: GrouperIndividualID,
        on_intersection: bool,
        world: &mut World,
    ) {
        self.asphalt_grouper.remove(lane, world);

        if !on_intersection {
            self.marker_grouper.remove(lane, world);
        }
    }

    pub fn on_unbuild_switch(&mut self, lane: GrouperIndividualID, world: &mut World) {
        self.gaps_grouper.remove(lane, world);
    }
}

use browser_ui::BrowserUI;

pub fn on_build(lane: &Lane, world: &mut World) {
    LaneRenderer::local_first(world).on_build(
        lane.id_as(),
        lane.connectivity.on_intersection,
        world,
    );

    BrowserUI::global_broadcast(world).on_lane_constructed(
        lane.id.as_raw(),
        lane.construction.path.clone(),
        false,
        lane.connectivity.on_intersection,
        world,
    );
}

pub fn on_build_switch(lane: &SwitchLane, world: &mut World) {
    LaneRenderer::local_first(world).on_build_switch(lane.id_as(), world);
    BrowserUI::global_broadcast(world).on_lane_constructed(
        lane.id.as_raw(),
        lane.construction.path.clone(),
        true,
        false,
        world,
    );
}

pub fn on_unbuild(lane: &Lane, world: &mut World) {
    LaneRenderer::local_first(world).on_unbuild(
        lane.id_as(),
        lane.connectivity.on_intersection,
        world,
    );

    BrowserUI::global_broadcast(world).on_lane_destructed(
        lane.id.as_raw(),
        false,
        lane.connectivity.on_intersection,
        world,
    );

    if DEBUG_VIEW_LANDMARKS {
        // TODO: move this to LaneRenderer
        Renderer::local_first(world).update_individual(
            RenderLayers::DebugLandmarkAssociation as u32 + lane.id.as_raw().instance_id as u32,
            Mesh::empty(),
            Instance::with_color([0.0, 0.0, 0.0]),
            true,
            world,
        );
    }

    if DEBUG_VIEW_SIGNALS {
        Renderer::local_first(world).update_individual(
            RenderLayers::DebugLandmarkAssociation as u32 + lane.id.as_raw().instance_id as u32,
            Mesh::empty(),
            Instance::with_color([0.0, 0.0, 0.0]),
            true,
            world,
        );
    }
}

pub fn on_unbuild_switch(lane: &SwitchLane, world: &mut World) {
    LaneRenderer::local_first(world).on_unbuild_switch(lane.id_as(), world);
    BrowserUI::global_broadcast(world).on_lane_destructed(lane.id.as_raw(), true, false, world);
}

mod kay_auto;
pub use self::kay_auto::*;
