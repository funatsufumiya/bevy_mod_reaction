use bevy::{
    ecs::{
        component::StorageType,
        query::{QueryData, QueryFilter},
        system::{SystemParam, SystemParamItem, SystemState},
        world::DeferredWorld,
    },
    prelude::*,
};
use std::{
    marker::PhantomData,
    mem,
    ops::Deref,
    sync::{Arc, Mutex},
};

pub trait ReactiveQueryData<F: QueryFilter>: QueryData + Sized {
    type State: Send + Sync + 'static;

    fn init(world: &mut World) -> <Self as ReactiveQueryData<F>>::State;

    fn is_changed(world: DeferredWorld, state: &mut <Self as ReactiveQueryData<F>>::State) -> bool;

    fn get<'w, 's>(
        world: &'w mut DeferredWorld<'w>,
        state: &'s mut <Self as ReactiveQueryData<F>>::State,
    ) -> Query<'w, 's, Self, F>;
}

impl<F, T> ReactiveQueryData<F> for &T
where
    F: QueryFilter + 'static,
    T: Component,
{
    type State = SystemState<(
        Query<'static, 'static, (), (Changed<T>, F)>,
        Query<'static, 'static, &'static T, F>,
    )>;

    fn init(world: &mut World) -> <Self as ReactiveQueryData<F>>::State {
        SystemState::new(world)
    }

    fn is_changed<'w>(
        world: DeferredWorld,
        state: &mut <Self as ReactiveQueryData<F>>::State,
    ) -> bool {
        !state.get(&world).0.is_empty()
    }

    fn get<'w, 's>(
        world: &'w mut DeferredWorld<'w>,
        state: &'s mut <Self as ReactiveQueryData<F>>::State,
    ) -> Query<'w, 's, Self, F> {
        // TODO verify safety
        unsafe { mem::transmute(state.get(&world).1) }
    }
}

pub trait ReactiveSystemParam: SystemParam {
    type State: Send + Sync + 'static;

    fn init(world: &mut World) -> <Self as ReactiveSystemParam>::State;

    fn is_changed(world: DeferredWorld, state: &mut <Self as ReactiveSystemParam>::State) -> bool;

    unsafe fn get<'w: 's, 's>(
        world: &'w mut DeferredWorld<'w>,
        state: &'s mut <Self as ReactiveSystemParam>::State,
    ) -> Self::Item<'w, 's>;
}

impl ReactiveSystemParam for Commands<'_, '_> {
    type State = ();

    fn init(world: &mut World) -> <Self as ReactiveSystemParam>::State {
        let _ = world;
    }

    fn is_changed(world: DeferredWorld, state: &mut <Self as ReactiveSystemParam>::State) -> bool {
        let _ = world;
        let _ = state;

        false
    }

    unsafe fn get<'w: 's, 's>(
        world: &'w mut DeferredWorld<'w>,
        state: &'s mut <Self as ReactiveSystemParam>::State,
    ) -> Self::Item<'w, 's> {
        let _ = state;

        world.commands()
    }
}

impl<R: Resource> ReactiveSystemParam for Res<'_, R> {
    type State = ();

    fn init(world: &mut World) -> <Self as ReactiveSystemParam>::State {
        let _ = world;
    }

    fn is_changed(world: DeferredWorld, state: &mut <Self as ReactiveSystemParam>::State) -> bool {
        let _ = state;
        world.resource_ref::<R>().is_changed()
    }

    unsafe fn get<'w: 's, 's>(
        world: &'w mut DeferredWorld<'w>,
        state: &'s mut <Self as ReactiveSystemParam>::State,
    ) -> Self::Item<'w, 's> {
        let _ = state;
        world.resource_ref::<R>()
    }
}

impl<D, F> ReactiveSystemParam for Query<'_, '_, D, F>
where
    D: ReactiveQueryData<F> + QueryData + 'static,
    F: QueryFilter + 'static,
{
    type State = <D as ReactiveQueryData<F>>::State;

    fn init(world: &mut World) -> <Self as ReactiveSystemParam>::State {
        <D as ReactiveQueryData<F>>::init(world)
    }

    fn is_changed<'a>(
        world: DeferredWorld,
        state: &mut <Self as ReactiveSystemParam>::State,
    ) -> bool {
        <D as ReactiveQueryData<F>>::is_changed(world, state)
    }

    unsafe fn get<'w: 's, 's>(
        world: &'w mut DeferredWorld<'w>,
        state: &'s mut <Self as ReactiveSystemParam>::State,
    ) -> Self::Item<'w, 's> {
        <D as ReactiveQueryData<F>>::get(world, state)
    }
}

impl<T: ReactiveSystemParam> ReactiveSystemParam for (T,) {
    type State = <T as ReactiveSystemParam>::State;

    fn init(world: &mut World) -> <Self as ReactiveSystemParam>::State {
        T::init(world)
    }

    fn is_changed<'a>(
        world: DeferredWorld,
        state: &mut <Self as ReactiveSystemParam>::State,
    ) -> bool {
        T::is_changed(world, state)
    }

    unsafe fn get<'w: 's, 's>(
        world: &'w mut DeferredWorld<'w>,
        state: &'s mut <Self as ReactiveSystemParam>::State,
    ) -> Self::Item<'w, 's> {
        (T::get(world, state),)
    }
}

impl<T1: ReactiveSystemParam, T2: ReactiveSystemParam> ReactiveSystemParam for (T1, T2) {
    type State = (
        <T1 as ReactiveSystemParam>::State,
        <T2 as ReactiveSystemParam>::State,
    );

    fn init(world: &mut World) -> <Self as ReactiveSystemParam>::State {
        (T1::init(world), T2::init(world))
    }

    fn is_changed<'a>(
        mut world: DeferredWorld,
        state: &mut <Self as ReactiveSystemParam>::State,
    ) -> bool {
        T1::is_changed(world.reborrow(), &mut state.0) || T2::is_changed(world, &mut state.1)
    }

    unsafe fn get<'w: 's, 's>(
        world: &'w mut DeferredWorld<'w>,
        state: &'s mut <Self as ReactiveSystemParam>::State,
    ) -> Self::Item<'w, 's> {
        let world_ptr = world as *mut _;
        (
            T1::get(unsafe { &mut *world_ptr }, &mut state.0),
            T2::get(unsafe { &mut *world_ptr }, &mut state.1),
        )
    }
}

pub struct Scope<T = ()> {
    pub entity: Entity,
    pub input: T,
}

impl<T> Deref for Scope<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.input
    }
}

pub trait ReactiveSystemParamFunction<Marker> {
    type Param: ReactiveSystemParam;

    type In;

    type Out;

    fn run(
        &mut self,
        param: SystemParamItem<Self::Param>,
        input: Self::In,
        entity: Entity,
    ) -> Self::Out;
}

impl<Marker, F, T> ReactiveSystemParamFunction<Marker> for F
where
    F: SystemParamFunction<Marker, In = Scope<T>>,
    F::Param: ReactiveSystemParam,
{
    type Param = F::Param;

    type In = T;

    type Out = F::Out;

    fn run(
        &mut self,
        param: SystemParamItem<Self::Param>,
        input: Self::In,
        entity: Entity,
    ) -> Self::Out {
        SystemParamFunction::run(self, Scope { entity, input }, param)
    }
}

pub trait ReactiveSystem: Send + Sync {
    type In;

    type Out;

    fn init(&mut self, world: &mut World);

    fn is_changed(&mut self, world: DeferredWorld) -> bool;

    fn run(&mut self, input: Self::In, world: DeferredWorld, entity: Entity) -> Self::Out;
}

pub struct FunctionReactiveSystem<F, S, Marker> {
    f: F,
    state: Option<S>,
    _marker: PhantomData<Marker>,
}

impl<F, S, Marker> ReactiveSystem for FunctionReactiveSystem<F, S, Marker>
where
    F: ReactiveSystemParamFunction<Marker> + Send + Sync,
    F::Param: ReactiveSystemParam<State = S>,
    S: Send + Sync,
    Marker: Send + Sync,
{
    type In = F::In;
    type Out = F::Out;

    fn init(&mut self, world: &mut World) {
        self.state = Some(F::Param::init(world));
    }

    fn is_changed(&mut self, world: DeferredWorld) -> bool {
        F::Param::is_changed(world, self.state.as_mut().unwrap())
    }

    fn run(&mut self, input: Self::In, mut world: DeferredWorld, entity: Entity) -> Self::Out {
        // TODO check for overlapping params
        let mut world = world.reborrow();
        let params = unsafe { F::Param::get(&mut world, self.state.as_mut().unwrap()) };

        self.f.run(params, input, entity)
    }
}

#[derive(Clone)]
pub struct Reaction {
    system: Arc<Mutex<Box<dyn ReactiveSystem<In = (), Out = ()>>>>,
}

impl Component for Reaction {
    const STORAGE_TYPE: StorageType = StorageType::Table;

    fn register_component_hooks(hooks: &mut bevy::ecs::component::ComponentHooks) {
        hooks.on_insert(|mut world, entity, _| {
            world.commands().add(move |world: &mut World| {
                let me = world
                    .query::<&Reaction>()
                    .get(world, entity)
                    .unwrap()
                    .clone();
                me.system.lock().unwrap().init(world);
            });
        });
    }
}

impl Reaction {
    pub fn new<Marker>(
        system: impl ReactiveSystemParamFunction<Marker, In = (), Out = ()> + Send + Sync + 'static,
    ) -> Self
    where
        Marker: Send + Sync + 'static,
    {
        Self {
            system: Arc::new(Mutex::new(Box::new(FunctionReactiveSystem {
                f: system,
                state: None,
                _marker: PhantomData,
            }))),
        }
    }
}

pub fn react(mut world: DeferredWorld, reaction_query: Query<(Entity, &Reaction)>) {
    for (entity, reaction) in &reaction_query {
        let mut system = reaction.system.lock().unwrap();

        if system.is_changed(world.reborrow()) {
            system.run((), world.reborrow(), entity);
        }
    }
}
